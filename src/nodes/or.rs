use std::collections::HashMap;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::{Message, Signal, Sink, Source};

/// A logic OR gate: emits `On` when **any** input is
/// `On`, and `Off` when **all** inputs are `Off`.
///
/// Starts `Off` and does not emit until the first
/// message is received.
///
/// ```rust,ignore
/// let (sink, source) = Or::new("o", sinks).split();
/// bus.add_sink(Box::new(sink));
/// bus.add_source(Box::new(source));
/// ```
pub struct Or {
    name: String,
    sinks: Vec<String>,
}

impl Or {
    pub fn new(
        name: impl Into<String>,
        sinks: Vec<String>,
    ) -> Self {
        Self { name: name.into(), sinks }
    }

    pub fn split(self) -> (OrSink, OrSource) {
        let (tx, rx) = mpsc::channel(16);
        (
            OrSink { name: self.name.clone(), tx },
            OrSource {
                name: self.name,
                sinks: self.sinks,
                rx,
            },
        )
    }
}

// ── sink half ─────────────────────────────────────────

pub struct OrSink {
    name: String,
    tx: mpsc::Sender<Signal>,
}

impl Sink for OrSink {
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
            let mut emitted = Signal::Off;

            while let Some(msg) = rx.recv().await {
                states.insert(
                    msg.source.clone(),
                    msg.signal,
                );

                let output = if states
                    .values()
                    .any(|&s| s == Signal::On)
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

// ── source half ───────────────────────────────────────

pub struct OrSource {
    name: String,
    sinks: Vec<String>,
    rx: mpsc::Receiver<Signal>,
}

impl Source for OrSource {
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
