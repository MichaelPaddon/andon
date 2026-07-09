use std::error::Error;
use std::fmt;

/// Displays an error together with its full `source()`
/// chain, separated by ": ".
///
/// `Display` on a wrapped error (reqwest in particular)
/// prints only the outermost layer, e.g. "error sending
/// request for url (...)", hiding the root cause (DNS
/// failure, connection refused, timeout). Logging via
/// this wrapper preserves the whole story:
///
/// ```text
/// error sending request for url (...): client error
/// (Connect): dns error: failed to lookup address
/// ```
pub struct ErrorChain<'a>(pub &'a (dyn Error + 'static));

impl fmt::Display for ErrorChain<'_> {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(f, "{}", self.0)?;
        let mut cause = self.0.source();
        while let Some(e) = cause {
            write!(f, ": {}", e)?;
            cause = e.source();
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct Leaf;

    impl fmt::Display for Leaf {
        fn fmt(
            &self,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            write!(f, "connection refused")
        }
    }

    impl Error for Leaf {}

    #[derive(Debug)]
    struct Wrapper(Leaf);

    impl fmt::Display for Wrapper {
        fn fmt(
            &self,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            write!(f, "request failed")
        }
    }

    impl Error for Wrapper {
        fn source(
            &self,
        ) -> Option<&(dyn Error + 'static)> {
            Some(&self.0)
        }
    }

    #[test]
    fn single_error_has_no_separator() {
        assert_eq!(
            ErrorChain(&Leaf).to_string(),
            "connection refused"
        );
    }

    #[test]
    fn chain_includes_all_causes() {
        assert_eq!(
            ErrorChain(&Wrapper(Leaf)).to_string(),
            "request failed: connection refused"
        );
    }
}
