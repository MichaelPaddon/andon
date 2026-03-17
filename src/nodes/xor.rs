use std::collections::HashMap;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::{Message, Signal, Sink, Source};

/// A logic XOR gate: emits `On` when an **odd** number
/// of inputs are `On`, `Off` otherwise.
///
/// Starts `Off` and does not emit until the first
/// message is received.
///
/// ```rust,ignore
/// let (sink, source) = Xor::new("x", sinks).split();
/// bus.add_sink(Box::new(sink));
/// bus.add_source(Box::new(source));
/// ```
pub struct Xor {
    name: String,
    sinks: Vec<String>,
}

impl Xor {
    pub fn new(
        name: impl Into<String>,
        sinks: Vec<String>,
    ) -> Self {
        Self { name: name.into(), sinks }
    }

    pub fn split(self) -> (XorSink, XorSource) {
        let (tx, rx) = mpsc::channel(16);
        (
            XorSink { name: self.name.clone(), tx },
            XorSource {
                name: self.name,
                sinks: self.sinks,
                rx,
            },
        )
    }
}

// ── sink half ─────────────────────────────────────────

pub struct XorSink {
    name: String,
    tx: mpsc::Sender<Signal>,
}

impl Sink for XorSink {
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

                let on_count = states
                    .values()
                    .filter(|&&s| s == Signal::On)
                    .count();
                let output = if on_count % 2 == 1 {
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

pub struct XorSource {
    name: String,
    sinks: Vec<String>,
    rx: mpsc::Receiver<Signal>,
}

impl Source for XorSource {
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
