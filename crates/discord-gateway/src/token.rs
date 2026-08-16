//! The bot token, in a wrapper that cannot print itself.
//!
//! This crate deliberately does not depend on `security`: it never stores a
//! token, decides an encryption key, or reads a row. What it does do is hold a
//! plaintext token in memory for as long as a connection lives, and pass it to
//! two places that must see it — the `Authorization` header and the gateway's
//! IDENTIFY.
//!
//! The risk that carries is not cryptographic, it is clerical. A token reaches
//! a log because a struct holding it derived `Debug` and something printed the
//! struct. `missing_debug_implementations` is a warning across this workspace,
//! so every type here has a `Debug` — which means the protection has to come
//! from what that `Debug` prints, not from its absence.

use std::fmt;

/// A Discord bot token.
///
/// `Debug` prints a placeholder, and there is no `Display`. Reaching the
/// plaintext takes an explicit [`BotToken::expose`], which greps as easily as
/// it reads.
#[derive(Clone, PartialEq, Eq)]
pub struct BotToken(String);

impl BotToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self(token.into())
    }

    /// The plaintext, for the two callers that need it.
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// The value of an `Authorization` header.
    pub fn header_value(&self) -> String {
        format!("Bot {}", self.0)
    }

    pub fn is_empty(&self) -> bool {
        self.0.trim().is_empty()
    }
}

impl fmt::Debug for BotToken {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("BotToken(«redacted»)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SECRET: &str = "MTIzNDU2Nzg5.GhIjKl.SUPER_SECRET_VALUE";

    #[test]
    fn debug_does_not_print_the_token() {
        let token = BotToken::new(SECRET);
        let printed = format!("{token:?}");

        assert!(!printed.contains("SUPER_SECRET_VALUE"), "leaked: {printed}");
        assert_eq!(printed, "BotToken(«redacted»)");
    }

    #[test]
    fn debug_of_a_containing_struct_does_not_print_it_either() {
        #[derive(Debug)]
        struct Connection {
            #[allow(dead_code, reason = "read only by the derived Debug")]
            token: BotToken,
        }

        let printed = format!(
            "{:?}",
            Connection {
                token: BotToken::new(SECRET)
            }
        );
        assert!(!printed.contains("SUPER_SECRET_VALUE"), "leaked: {printed}");
    }

    #[test]
    fn the_header_carries_the_bot_prefix() {
        assert_eq!(
            BotToken::new("abc").header_value(),
            "Bot abc",
            "Discord rejects a bare token"
        );
    }

    #[test]
    fn whitespace_only_is_empty() {
        assert!(BotToken::new("   ").is_empty());
        assert!(!BotToken::new("abc").is_empty());
    }
}
