use std::str::FromStr;
use std::time::Duration;

use chrono::Local;
use cron::Schedule;
use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, warn};

use crate::{Signal, Source};

/// Emits `On` at times matching a cron schedule and
/// `Off` after a fixed duration has elapsed.
///
/// The cron expression uses a 6-field format:
/// `sec  min  hour  day-of-month  month  day-of-week`
///
/// Times are interpreted in the local timezone.
///
/// If the `On` period has not yet ended when the next
/// scheduled time arrives, the current period runs to
/// completion before the next one begins.
pub struct CronSource {
    name: String,
    schedule: Schedule,
    duration: Duration,
    sinks: Vec<String>,
}

impl CronSource {
    pub fn new(
        name: impl Into<String>,
        spec: &str,
        duration: Duration,
        sinks: Vec<String>,
    ) -> Result<Self, String> {
        let schedule = Schedule::from_str(spec)
            .map_err(|e| {
                format!("invalid cron spec {:?}: {}", spec, e)
            })?;
        Ok(Self {
            name: name.into(),
            schedule,
            duration,
            sinks,
        })
    }
}

impl Source for CronSource {
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
            let mut upcoming =
                self.schedule.upcoming(Local);

            loop {
                let next = match upcoming.next() {
                    Some(t) => t,
                    None => {
                        warn!(
                            source = %self.name,
                            "cron schedule exhausted"
                        );
                        break;
                    }
                };

                let wait = (next - Local::now())
                    .to_std()
                    .unwrap_or_default();

                debug!(
                    source = %self.name,
                    at     = %next,
                    "next cron firing"
                );
                tokio::time::sleep(wait).await;

                debug!(source = %self.name, "firing on");
                if tx.send(Signal::On).await.is_err() {
                    break;
                }

                tokio::time::sleep(self.duration).await;

                debug!(source = %self.name, "firing off");
                if tx.send(Signal::Off).await.is_err() {
                    break;
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_spec_is_a_helpful_error() {
        let err = CronSource::new(
            "c",
            "bogus",
            Duration::from_secs(1),
            vec![],
        )
        .err()
        .expect("bogus spec should fail");
        assert!(err.contains("invalid cron spec"));
    }

    #[tokio::test(start_paused = true)]
    async fn fires_on_then_off() {
        // Every second, on for one second. The paused
        // clock auto-advances through the sleeps.
        let src = CronSource::new(
            "c",
            "* * * * * *",
            Duration::from_secs(1),
            vec![],
        )
        .expect("valid spec");

        let (tx, mut rx) = mpsc::channel(16);
        Box::new(src).start(tx);

        assert_eq!(rx.recv().await, Some(Signal::On));
        assert_eq!(rx.recv().await, Some(Signal::Off));
    }
}
