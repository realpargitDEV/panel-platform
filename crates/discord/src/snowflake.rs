//! Discord identifiers.
//!
//! A snowflake is a 64-bit integer. That matters more than it looks: they are
//! routinely above 2^53, which is the largest integer a JavaScript `number` can
//! hold exactly. Sending one to the frontend as a JSON number silently corrupts
//! the last digits, and the resulting bug looks like "Discord says that channel
//! does not exist" rather than anything numeric.
//!
//! So a [`Snowflake`] serialises as a string, always, in both directions.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// A Discord guild, channel, user, role or message identifier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Snowflake(u64);

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SnowflakeError {
    #[error("a Discord id must not be empty")]
    Empty,
    #[error("a Discord id must be digits only, got {0:?}")]
    NotNumeric(String),
    #[error("a Discord id must not be zero")]
    Zero,
}

impl Snowflake {
    pub fn new(value: u64) -> Result<Self, SnowflakeError> {
        if value == 0 {
            return Err(SnowflakeError::Zero);
        }
        Ok(Self(value))
    }

    pub fn get(self) -> u64 {
        self.0
    }
}

impl FromStr for Snowflake {
    type Err = SnowflakeError;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return Err(SnowflakeError::Empty);
        }
        // Rejecting a leading `+` or `-` here rather than letting `parse`
        // decide, so that "-1" is a clear "not numeric" instead of an overflow.
        if !trimmed.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(SnowflakeError::NotNumeric(trimmed.to_string()));
        }
        let value: u64 = trimmed
            .parse()
            .map_err(|_| SnowflakeError::NotNumeric(trimmed.to_string()))?;
        Snowflake::new(value)
    }
}

impl fmt::Display for Snowflake {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for Snowflake {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}

impl<'de> Deserialize<'de> for Snowflake {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        // Accepts a number too, because Discord's own API sends them as strings
        // but a hand-written config file will not.
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Raw {
            Text(String),
            Number(u64),
        }

        match Raw::deserialize(deserializer)? {
            Raw::Text(text) => text.parse().map_err(serde::de::Error::custom),
            Raw::Number(value) => Snowflake::new(value).map_err(serde::de::Error::custom),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole reason this type exists.
    #[test]
    fn a_snowflake_serialises_as_a_string_not_a_number() {
        // A real-shaped id, comfortably above 2^53.
        let id = Snowflake::new(1_234_567_890_123_456_789).expect("non-zero");
        let json = serde_json::to_string(&id).expect("serialise");
        assert_eq!(json, "\"1234567890123456789\"");
    }

    #[test]
    fn a_large_snowflake_survives_a_round_trip_exactly() {
        // As a JSON number this value would come back as
        // 1234567890123456800 in any JavaScript consumer.
        let id = Snowflake::new(1_234_567_890_123_456_789).expect("non-zero");
        let json = serde_json::to_string(&id).expect("serialise");
        let back: Snowflake = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(back, id);
        assert_eq!(back.get(), 1_234_567_890_123_456_789);
    }

    #[test]
    fn a_json_number_is_still_accepted() {
        let back: Snowflake = serde_json::from_str("1234567890123456789").expect("deserialise");
        assert_eq!(back.get(), 1_234_567_890_123_456_789);
    }

    #[test]
    fn zero_is_refused() {
        // Discord uses 0 for "unset" in some payloads; treating it as a real id
        // would send messages to a channel that cannot exist.
        assert_eq!(Snowflake::new(0), Err(SnowflakeError::Zero));
        assert_eq!("0".parse::<Snowflake>(), Err(SnowflakeError::Zero));
    }

    #[test]
    fn non_numeric_text_is_refused() {
        for bad in ["", "  ", "abc", "12a", "-1", "+1", "1.0", "12 34"] {
            assert!(
                bad.parse::<Snowflake>().is_err(),
                "{bad:?} should not parse"
            );
        }
    }

    #[test]
    fn surrounding_whitespace_is_tolerated() {
        // Users paste ids out of the Discord client, and the client is generous
        // with trailing spaces.
        assert_eq!(
            "  123456789012345678  "
                .parse::<Snowflake>()
                .expect("parse"),
            Snowflake::new(123_456_789_012_345_678).expect("non-zero")
        );
    }

    #[test]
    fn a_value_too_large_for_u64_is_refused_rather_than_wrapping() {
        let too_big = "99999999999999999999999";
        assert!(too_big.parse::<Snowflake>().is_err());
    }
}
