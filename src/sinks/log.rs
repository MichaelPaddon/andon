use tokio::{sync::mpsc, task::JoinHandle};
use tracing::info;

use crate::{Message, Signal, Sink};

/// Logs every signal change to stderr.
pub struct LogSink {
    name: String,
}

impl LogSink {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

impl Sink for LogSink {
    fn name(&self) -> &str {
        &self.name
    }

    fn start(
        self: Box<Self>,
        mut rx: mpsc::Receiver<Message>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                let state = match msg.signal {
                    Signal::On => "on",
                    Signal::Off => "off",
                };
                info!(
                    sink   = %self.name,
                    source = %msg.source,
                    signal = state,
                );
            }
        })
    }
}
