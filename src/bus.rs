use std::collections::HashMap;

use tokio::sync::mpsc;
use tracing::info;

use crate::{Alarm, Message, Signal, Sink, Source};

/// Returned by [`Bus::run`]; call [`shutdown`] to clear
/// all alarms before the process exits.
///
/// [`shutdown`]: ShutdownHandle::shutdown
pub struct ShutdownHandle {
    alarms: Vec<Box<dyn Alarm>>,
}

impl ShutdownHandle {
    pub async fn shutdown(self) {
        if self.alarms.is_empty() {
            return;
        }
        info!("clearing alarms");
        for alarm in &self.alarms {
            alarm.clear().await;
        }
    }
}

/// Wires sources to sinks by name and runs them.
pub struct Bus {
    sources: Vec<Box<dyn Source>>,
    sinks: HashMap<String, Box<dyn Sink>>,
    alarms: Vec<Box<dyn Alarm>>,
}

impl Bus {
    pub fn new() -> Self {
        Self {
            sources: Vec::new(),
            sinks: HashMap::new(),
            alarms: Vec::new(),
        }
    }

    pub fn add_source(&mut self, source: Box<dyn Source>) {
        self.sources.push(source);
    }

    pub fn add_sink(&mut self, sink: Box<dyn Sink>) {
        self.sinks.insert(sink.name().to_owned(), sink);
    }

    pub fn add_alarm(&mut self, alarm: Box<dyn Alarm>) {
        self.alarms.push(alarm);
    }

    /// Start all sources and sinks.
    ///
    /// Clears all registered alarms before any source
    /// begins emitting.  Returns a [`ShutdownHandle`]
    /// that clears alarms again on shutdown.
    ///
    /// Returns an error listing any sink names declared
    /// by sources that were never registered.
    ///
    /// Each source gets a raw `Signal` channel.  A relay
    /// task per source wraps each signal in a `Message`
    /// (tagging it with the source name) and fans it out
    /// to every declared sink.
    pub async fn run(
        mut self,
    ) -> Result<
        (Vec<tokio::task::JoinHandle<()>>, ShutdownHandle),
        Vec<String>,
    > {
        let missing: Vec<String> = self
            .sources
            .iter()
            .flat_map(|s| s.sink_names().iter().cloned())
            .filter(|n| !self.sinks.contains_key(n))
            .collect();

        if !missing.is_empty() {
            return Err(missing);
        }

        // Clear alarms before sources start emitting.
        if !self.alarms.is_empty() {
            info!("clearing alarms at startup");
            for alarm in &self.alarms {
                alarm.clear().await;
            }
        }

        // Initialise all sink connections sequentially
        // so every sink is ready before sources start.
        info!("initializing connections");
        for sink in self.sinks.values_mut() {
            sink.init().await;
        }

        const CAP: usize = 16;

        let mut handles = Vec::new();
        let mut sink_txs: HashMap<
            String,
            mpsc::Sender<Message>,
        > = HashMap::new();

        for (name, sink) in self.sinks {
            let (tx, rx) = mpsc::channel(CAP);
            sink_txs.insert(name, tx);
            handles.push(sink.start(rx));
        }

        for source in self.sources {
            let source_name = source.name().to_owned();
            let sink_names: Vec<String> =
                source.sink_names().to_vec();
            let txs: Vec<mpsc::Sender<Message>> =
                sink_names
                    .iter()
                    .filter_map(|n| {
                        sink_txs.get(n).cloned()
                    })
                    .collect();

            let (src_tx, mut src_rx) =
                mpsc::channel::<Signal>(CAP);

            handles.push(source.start(src_tx));

            handles.push(tokio::spawn(async move {
                while let Some(sig) =
                    src_rx.recv().await
                {
                    let msg = Message {
                        source: source_name.clone(),
                        signal: sig,
                    };
                    for tx in &txs {
                        let _ =
                            tx.send(msg.clone()).await;
                    }
                }
            }));
        }

        Ok((handles, ShutdownHandle { alarms: self.alarms }))
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use tokio::task::JoinHandle;

    use super::*;
    use crate::Sink as SinkTrait;

    /// Emits a fixed sequence of signals, then idles so
    /// the bus relay task stays alive.
    struct SeqSource {
        name: String,
        sinks: Vec<String>,
        seq: Vec<Signal>,
    }

    impl Source for SeqSource {
        fn name(&self) -> &str {
            &self.name
        }

        fn sink_names(&self) -> &[String] {
            &self.sinks
        }

        fn start(
            self: Box<Self>,
            tx: mpsc::Sender<Signal>,
        ) -> JoinHandle<()> {
            tokio::spawn(async move {
                for sig in self.seq {
                    if tx.send(sig).await.is_err() {
                        break;
                    }
                }
            })
        }
    }

    /// Forwards every received message to a test-side
    /// channel for assertions.
    struct CaptureSink {
        name: String,
        out: mpsc::Sender<Message>,
    }

    impl SinkTrait for CaptureSink {
        fn name(&self) -> &str {
            &self.name
        }

        fn start(
            self: Box<Self>,
            mut rx: mpsc::Receiver<Message>,
        ) -> JoinHandle<()> {
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if self.out.send(msg).await.is_err()
                    {
                        break;
                    }
                }
            })
        }
    }

    #[tokio::test]
    async fn unknown_sink_names_are_reported() {
        let mut bus = Bus::new();
        bus.add_source(Box::new(SeqSource {
            name: "s".into(),
            sinks: vec!["nope".into()],
            seq: vec![],
        }));

        let missing = bus
            .run()
            .await
            .err()
            .expect("run should fail");
        assert_eq!(missing, vec!["nope".to_owned()]);
    }

    #[tokio::test]
    async fn messages_are_tagged_and_fanned_out() {
        let (out_a, mut rx_a) = mpsc::channel(16);
        let (out_b, mut rx_b) = mpsc::channel(16);

        let mut bus = Bus::new();
        bus.add_sink(Box::new(CaptureSink {
            name: "a".into(),
            out: out_a,
        }));
        bus.add_sink(Box::new(CaptureSink {
            name: "b".into(),
            out: out_b,
        }));
        bus.add_source(Box::new(SeqSource {
            name: "src".into(),
            sinks: vec!["a".into(), "b".into()],
            seq: vec![Signal::On, Signal::Off],
        }));

        let (_handles, _shutdown) =
            bus.run().await.expect("run should succeed");

        for rx in [&mut rx_a, &mut rx_b] {
            let m1 =
                rx.recv().await.expect("first message");
            assert_eq!(m1.source, "src");
            assert_eq!(m1.signal, Signal::On);

            let m2 =
                rx.recv().await.expect("second message");
            assert_eq!(m2.signal, Signal::Off);
        }
    }
}
