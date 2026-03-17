/// A binary signal: on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    On,
    Off,
}

/// A signal tagged with the name of its source.
///
/// Sinks receive `Message` values so they can track
/// per-source state independently.
#[derive(Debug, Clone)]
pub struct Message {
    pub source: String,
    pub signal: Signal,
}
