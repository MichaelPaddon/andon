use std::collections::HashMap;

use tokio::{sync::mpsc, task::JoinHandle};
use tracing::{debug, info};

use crate::{Message, Signal, Sink};

/// Logs signal transitions to stderr.
///
/// Sources re-emit their current level on every check,
/// so logging every message would flood the log with
/// identical lines. Transitions (and the first signal
/// from each source) are logged at INFO; repeats are
/// demoted to DEBUG.
pub struct LogSink {
    name: String,
}

impl LogSink {
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into() }
    }
}

/// Outcome of observing a signal from a source.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    /// First signal ever seen from this source.
    Initial,
    /// Signal differs from the previous one.
    Changed { previous: Signal },
    /// Same signal as last time.
    Unchanged,
}

/// Remembers the last signal seen per source so that
/// repeats can be told apart from real transitions.
#[derive(Default)]
struct StateTracker {
    last: HashMap<String, Signal>,
}

impl StateTracker {
    fn observe(
        &mut self,
        source: &str,
        signal: Signal,
    ) -> Change {
        match self.last.insert(source.to_owned(), signal)
        {
            None => Change::Initial,
            Some(prev) if prev != signal => {
                Change::Changed { previous: prev }
            }
            Some(_) => Change::Unchanged,
        }
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
            let mut tracker = StateTracker::default();

            while let Some(msg) = rx.recv().await {
                let change = tracker
                    .observe(&msg.source, msg.signal);
                match change {
                    Change::Initial => info!(
                        sink   = %self.name,
                        source = %msg.source,
                        signal = msg.signal.as_str(),
                        "initial signal",
                    ),
                    Change::Changed { previous } => {
                        info!(
                            sink     = %self.name,
                            source   = %msg.source,
                            signal   = msg.signal
                                .as_str(),
                            previous = previous.as_str(),
                            "signal changed",
                        )
                    }
                    Change::Unchanged => debug!(
                        sink   = %self.name,
                        source = %msg.source,
                        signal = msg.signal.as_str(),
                        "signal unchanged",
                    ),
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_signal_is_initial() {
        let mut t = StateTracker::default();
        assert_eq!(
            t.observe("a", Signal::Off),
            Change::Initial
        );
    }

    #[test]
    fn repeat_is_unchanged() {
        let mut t = StateTracker::default();
        t.observe("a", Signal::Off);
        assert_eq!(
            t.observe("a", Signal::Off),
            Change::Unchanged
        );
    }

    #[test]
    fn transition_reports_previous() {
        let mut t = StateTracker::default();
        t.observe("a", Signal::Off);
        assert_eq!(
            t.observe("a", Signal::On),
            Change::Changed { previous: Signal::Off }
        );
    }

    #[test]
    fn sources_tracked_independently() {
        let mut t = StateTracker::default();
        t.observe("a", Signal::On);
        assert_eq!(
            t.observe("b", Signal::On),
            Change::Initial
        );
        assert_eq!(
            t.observe("a", Signal::On),
            Change::Unchanged
        );
    }
}
