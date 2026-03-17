use std::future::Future;
use std::time::Duration;

use rand_distr::{Distribution, Normal};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::{Signal, Source};

// ── schedule ─────────────────────────────────────────

/// Timing schedule for a probe.
///
/// With no stddev, every interval is identical.
/// With stddev, each interval is sampled from
/// Normal(interval, stddev) and clamped to zero.
pub struct ProbeSchedule {
    interval: Duration,
    stddev: Option<Duration>,
}

impl ProbeSchedule {
    pub fn new(interval: Duration) -> Self {
        Self { interval, stddev: None }
    }

    pub fn with_stddev(mut self, sd: Duration) -> Self {
        self.stddev = Some(sd);
        self
    }

    /// Return the duration to wait before the next probe.
    pub fn next_sleep(&self) -> Duration {
        match self.stddev {
            None => self.interval,
            Some(sd) => {
                let dist = Normal::new(
                    self.interval.as_secs_f64(),
                    sd.as_secs_f64(),
                )
                .expect("stddev must be finite and >= 0");
                let secs = dist
                    .sample(&mut rand::thread_rng())
                    .max(0.0);
                Duration::from_secs_f64(secs)
            }
        }
    }
}

// ── trait ─────────────────────────────────────────────

/// A [`Source`] that runs on a [`ProbeSchedule`].
///
/// Implement this trait instead of [`Source`] directly.
/// The tick loop (including Gaussian jitter) is provided
/// via a blanket [`Source`] impl.
///
/// The first check fires immediately on start; subsequent
/// checks wait for the next scheduled interval.
pub trait Probe: Send + 'static {
    fn name(&self) -> &str;
    fn sink_names(&self) -> &[String];
    fn schedule(&self) -> &ProbeSchedule;

    /// Perform one check and return the resulting signal.
    fn check(
        &mut self,
    ) -> impl Future<Output = Signal> + Send + '_;
}

// ── blanket Source impl ───────────────────────────────

impl<P: Probe> Source for P {
    fn name(&self) -> &str {
        Probe::name(self)
    }

    fn sink_names(&self) -> &[String] {
        Probe::sink_names(self)
    }

    fn start(
        self: Box<Self>,
        tx: mpsc::Sender<Signal>,
    ) -> JoinHandle<()> {
        tokio::spawn(async move {
            let mut this = *self;
            loop {
                let sig = this.check().await;
                if tx.send(sig).await.is_err() {
                    break;
                }
                let wait = this.schedule().next_sleep();
                tokio::time::sleep(wait).await;
            }
        })
    }
}
