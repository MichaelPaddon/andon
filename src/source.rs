use tokio::{sync::mpsc, task::JoinHandle};

use crate::Signal;

/// A node that produces binary signals.
///
/// Sinks are declared by name at construction time;
/// the [`Bus`] resolves names to live channels before
/// calling [`Source::start`].  The bus tags outgoing
/// signals with `name()` before forwarding them.
///
/// [`Bus`]: crate::Bus
pub trait Source: Send + 'static {
    /// Unique name for this source in the system.
    fn name(&self) -> &str;

    /// Names of the sinks this source drives.
    fn sink_names(&self) -> &[String];

    /// Spawn a task that runs the source.
    ///
    /// `tx` delivers every emitted signal to the bus,
    /// which tags it with `name()` and fans it out to
    /// the declared sinks.
    fn start(
        self: Box<Self>,
        tx: mpsc::Sender<Signal>,
    ) -> JoinHandle<()>;
}
