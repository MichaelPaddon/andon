use std::collections::HashMap;
use std::pin::pin;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Instant};

use crate::{Message, Signal, Sink, Source};

/// A turn-on delay gate.
///
/// Emits `On` only after all inputs have been
/// continuously `On` for the configured delay.
/// Emits `Off` immediately when all inputs go `Off`.
///
/// If any input goes `Off` before the delay elapses,
/// the timer is cancelled and no `On` is emitted.
///
/// ```rust,ignore
/// let (sink, source) =
///     Delay::new("d", Duration::from_secs(30), sinks)
///         .split();
/// bus.add_sink(Box::new(sink));
/// bus.add_source(Box::new(source));
/// ```
pub struct Delay {
    name: String,
    delay: Duration,
    sinks: Vec<String>,
}

impl Delay {
    pub fn new(
        name: impl Into<String>,
        delay: Duration,
        sinks: Vec<String>,
    ) -> Self {
        Self { name: name.into(), delay, sinks }
    }

    pub fn split(self) -> (DelaySink, DelaySource) {
        let (tx, rx) = mpsc::channel(16);
        (
            DelaySink {
                name: self.name.clone(),
                delay: self.delay,
                tx,
            },
            DelaySource {
                name: self.name,
                sinks: self.sinks,
                rx,
            },
        )
    }
}

// ── sink half ────────────────────────────────────────

pub struct DelaySink {
    name: String,
    delay: Duration,
    tx: mpsc::Sender<Signal>,
}

impl Sink for DelaySink {
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
            let mut armed = false;
            let delay = self.delay;

            // Timer is only polled when `armed` is true,
            // so the initial deadline doesn't matter.
            let mut timer = pin!(sleep(Duration::ZERO));

            loop {
                tokio::select! {
                    // Delay elapsed — emit On if still
                    // warranted.
                    _ = &mut timer, if armed => {
                        armed = false;
                        if states.values()
                            .any(|&s| s == Signal::On)
                            && emitted == Signal::Off
                        {
                            emitted = Signal::On;
                            if self.tx
                                .send(Signal::On)
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                    }

                    msg = rx.recv() => {
                        let Some(msg) = msg else {
                            break;
                        };
                        states.insert(
                            msg.source.clone(),
                            msg.signal,
                        );
                        let any_on = states
                            .values()
                            .any(|&s| s == Signal::On);

                        if any_on
                            && emitted == Signal::Off
                            && !armed
                        {
                            // Arm the timer.
                            timer.as_mut().reset(
                                Instant::now() + delay,
                            );
                            armed = true;
                        } else if !any_on {
                            // Disarm and turn off at once.
                            armed = false;
                            if emitted == Signal::On {
                                emitted = Signal::Off;
                                if self.tx
                                    .send(Signal::Off)
                                    .await
                                    .is_err()
                                {
                                    break;
                                }
                            }
                        }
                    }
                }
            }
        })
    }
}

// ── source half ──────────────────────────────────────

pub struct DelaySource {
    name: String,
    sinks: Vec<String>,
    rx: mpsc::Receiver<Signal>,
}

impl Source for DelaySource {
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
