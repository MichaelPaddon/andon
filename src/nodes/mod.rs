mod and;
mod delay;
mod not;
mod or;
mod xor;

pub use and::{And, AndSink, AndSource};
pub use delay::{Delay, DelaySink, DelaySource};
pub use not::{Not, NotSink, NotSource};
pub use or::{Or, OrSink, OrSource};
pub use xor::{Xor, XorSink, XorSource};

#[cfg(test)]
pub(crate) mod testutil {
    use std::time::Duration;

    use tokio::sync::mpsc;

    use crate::{Message, Signal, Sink, Source};

    /// Drives a split node in tests: messages go in
    /// through the sink half, signals come out of the
    /// source half.
    ///
    /// Tests should run with a paused tokio clock
    /// (`#[tokio::test(start_paused = true)]`) so the
    /// timeouts below elapse instantly instead of
    /// slowing the suite down.
    pub struct Harness {
        tx: mpsc::Sender<Message>,
        rx: mpsc::Receiver<Signal>,
    }

    impl Harness {
        pub fn start(
            sink: impl Sink,
            source: impl Source,
        ) -> Self {
            let (msg_tx, msg_rx) = mpsc::channel(16);
            let (sig_tx, sig_rx) = mpsc::channel(16);
            Box::new(sink).start(msg_rx);
            Box::new(source).start(sig_tx);
            Self { tx: msg_tx, rx: sig_rx }
        }

        pub async fn send(
            &self,
            source: &str,
            signal: Signal,
        ) {
            self.tx
                .send(Message {
                    source: source.to_owned(),
                    signal,
                })
                .await
                .expect("node dropped its receiver");
        }

        /// Next output, failing the test if the node
        /// stays silent.
        pub async fn recv(&mut self) -> Signal {
            tokio::time::timeout(
                Duration::from_secs(3600),
                self.rx.recv(),
            )
            .await
            .expect("timed out waiting for node output")
            .expect("node output channel closed")
        }

        /// Assert the node emits nothing, even given
        /// (paused-clock) time for timers to fire.
        pub async fn assert_silent(&mut self) {
            let res = tokio::time::timeout(
                Duration::from_secs(3600),
                self.rx.recv(),
            )
            .await;
            assert!(
                res.is_err(),
                "unexpected node output: {:?}",
                res,
            );
        }
    }
}
