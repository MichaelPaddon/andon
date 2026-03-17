use std::future::Future;
use std::pin::Pin;

/// A sink that can be unconditionally silenced.
///
/// All registered alarms are cleared at startup (before
/// any source begins emitting) and again at shutdown.
pub trait Alarm: Send + 'static {
    /// Turn the alarm off regardless of current state.
    fn clear(
        &self,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + '_>>;
}
