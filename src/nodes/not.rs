use std::collections::HashMap;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::{Message, Signal, Sink, Source};

/// A logic NOT gate: emits `On` when **all** inputs are
/// `Off`, and `Off` when **any** input is `On`.
///
/// Because it is both a source and a sink, it is split
/// into two connected halves.  Add both to the [`Bus`]:
///
/// ```rust,ignore
/// let (sink, source) = Not::new("not1", sinks).split();
/// bus.add_sink(Box::new(sink));
/// bus.add_source(Box::new(source));
/// ```
///
/// [`Bus`]: crate::Bus
pub struct Not {
    name: String,
    sinks: Vec<String>,
}

impl Not {
    pub fn new(
        name: impl Into<String>,
        sinks: Vec<String>,
    ) -> Self {
        Self { name: name.into(), sinks }
    }

    /// Split into connected sink and source halves.
    pub fn split(self) -> (NotSink, NotSource) {
        let (tx, rx) = mpsc::channel(16);
        (
            NotSink { name: self.name.clone(), tx },
            NotSource { name: self.name, sinks: self.sinks, rx },
        )
    }
}

// ── sink half ────────────────────────────────────────

pub struct NotSink {
    name: String,
    tx: mpsc::Sender<Signal>,
}

impl Sink for NotSink {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(
        self: Box<Self>,
        mut rx: mpsc::Receiver<Message>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut states: HashMap<String, Signal> =
                HashMap::new();

            // No inputs → all off → output is On.
            // Emit that immediately so downstream sinks
            // see the correct initial state even when no
            // sources are connected.
            let mut emitted = Signal::On;
            if self.tx.send(Signal::On).await.is_err() {
                return;
            }

            while let Some(msg) = rx.recv().await {
                states
                    .insert(msg.source.clone(), msg.signal);

                let output = if states
                    .values()
                    .all(|&s| s == Signal::Off)
                {
                    Signal::On
                } else {
                    Signal::Off
                };

                if output != emitted {
                    emitted = output;
                    if self.tx.send(output).await.is_err()
                    {
                        break;
                    }
                }
            }
        })
    }
}

// ── source half ──────────────────────────────────────

pub struct NotSource {
    name: String,
    sinks: Vec<String>,
    rx: mpsc::Receiver<Signal>,
}

impl Source for NotSource {
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
            let mut rx = self.rx;
            while let Some(sig) = rx.recv().await {
                if tx.send(sig).await.is_err() {
                    break;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::nodes::testutil::Harness;

    fn harness() -> Harness {
        let (sink, src) =
            Not::new("not", vec![]).split();
        Harness::start(sink, src)
    }

    #[tokio::test(start_paused = true)]
    async fn emits_on_at_startup() {
        // No inputs means "all off", which inverts to On.
        let mut h = harness();
        assert_eq!(h.recv().await, Signal::On);
    }

    #[tokio::test(start_paused = true)]
    async fn any_on_input_inverts_to_off() {
        let mut h = harness();
        assert_eq!(h.recv().await, Signal::On);
        h.send("a", Signal::On).await;
        assert_eq!(h.recv().await, Signal::Off);
    }

    #[tokio::test(start_paused = true)]
    async fn all_off_inverts_back_to_on() {
        let mut h = harness();
        assert_eq!(h.recv().await, Signal::On);
        h.send("a", Signal::On).await;
        assert_eq!(h.recv().await, Signal::Off);
        h.send("a", Signal::Off).await;
        assert_eq!(h.recv().await, Signal::On);
    }

    #[tokio::test(start_paused = true)]
    async fn off_input_repeats_initial_state_silently() {
        let mut h = harness();
        assert_eq!(h.recv().await, Signal::On);
        h.send("a", Signal::Off).await;
        h.assert_silent().await;
    }
}
