/// A binary signal: on or off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    On,
    Off,
}

impl Signal {
    /// Lowercase name, convenient for log fields.
    pub fn as_str(self) -> &'static str {
        match self {
            Signal::On => "on",
            Signal::Off => "off",
        }
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn as_str_names_both_states() {
        assert_eq!(Signal::On.as_str(), "on");
        assert_eq!(Signal::Off.as_str(), "off");
    }
}
