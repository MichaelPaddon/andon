use std::future::Future;
use std::pin::Pin;

use tokio::{sync::mpsc, task::JoinHandle};

use crate::Message;

/// A node that consumes binary signals.
///
/// Each sink has a unique name. The [`Bus`] uses that
/// name to wire source outputs to the correct sink.
///
/// [`Bus`]: crate::Bus
pub trait Sink: Send + 'static {
    /// Unique name for this sink in the system.
    fn name(&self) -> &str;

    /// Establish external connections before the bus
    /// starts routing signals.
    ///
    /// Called once per sink, sequentially, before any
    /// source is started.  Log errors internally; the
    /// bus does not inspect the outcome.  The default
    /// implementation is a no-op.
    fn init(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>> {
        Box::pin(async {})
    }

    /// Spawn a task that runs the sink.
    ///
    /// `rx` delivers signals routed from any source
    /// that named this sink.
    fn start(
        self: Box<Self>,
        rx: mpsc::Receiver<Message>,
    ) -> JoinHandle<()>;
}
