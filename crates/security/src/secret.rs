//! A value that must never be printed.
//!
//! This is the primary control against secret disclosure, not the log
//! redaction in `agent-core`. Redaction by field name is a guess about intent;
//! this is a type with no formatting implementation that reveals its contents.
//! A secret cannot be logged by accident because `format!("{secret:?}")` simply
//! does not print it.

use std::fmt;

use zeroize::{Zeroize, ZeroizeOnDrop};

/// Wraps a sensitive value. `Debug` and `Display` print `[redacted]`, and the
/// buffer is zeroed when dropped so it does not linger in freed memory.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Secret<T: Zeroize>(T);

impl<T: Zeroize> Secret<T> {
    pub fn new(value: T) -> Self {
        Self(value)
    }

    /// Read the value. Named so that every use is greppable in review — the
    /// question "where do secrets get read?" has an exact answer.
    pub fn expose(&self) -> &T {
        &self.0
    }

    /// Consume the wrapper and take ownership of the value.
    pub fn into_inner(mut self) -> T
    where
        T: Default,
    {
        std::mem::take(&mut self.0)
    }
}

impl<T: Zeroize> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

impl<T: Zeroize> fmt::Display for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[redacted]")
    }
}

/// Deliberately not `Serialize`: a secret must never reach a response body or
/// a log sink through serialisation. `From` is provided so construction stays
/// convenient and there is no incentive to hold the raw value instead.
impl From<String> for Secret<String> {
    fn from(value: String) -> Self {
        Secret::new(value)
    }
}

impl From<Vec<u8>> for Secret<Vec<u8>> {
    fn from(value: Vec<u8>) -> Self {
        Secret::new(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_does_not_print_the_value() {
        let secret = Secret::new("hunter2".to_string());
        assert_eq!(format!("{secret:?}"), "[redacted]");
        assert!(!format!("{secret:?}").contains("hunter2"));
    }

    #[test]
    fn display_does_not_print_the_value() {
        let secret = Secret::new("hunter2".to_string());
        assert_eq!(format!("{secret}"), "[redacted]");
    }

    #[test]
    fn a_secret_nested_in_a_struct_stays_redacted() {
        // The realistic leak: a config struct derives Debug and someone logs
        // the whole thing.
        #[derive(Debug)]
        #[allow(dead_code)]
        struct Config {
            host: String,
            password: Secret<String>,
        }

        let config = Config {
            host: "localhost".to_string(),
            password: Secret::new("hunter2".to_string()),
        };
        let printed = format!("{config:?}");
        assert!(printed.contains("localhost"), "{printed}");
        assert!(!printed.contains("hunter2"), "secret leaked: {printed}");
    }

    #[test]
    fn expose_returns_the_value() {
        let secret = Secret::new("hunter2".to_string());
        assert_eq!(secret.expose(), "hunter2");
    }

    #[test]
    fn into_inner_yields_ownership() {
        let secret = Secret::new(vec![1u8, 2, 3]);
        assert_eq!(secret.into_inner(), vec![1, 2, 3]);
    }
}
