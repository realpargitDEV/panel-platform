//! Structured logging.
//!
//! Production writes JSON to a daily-rotated file; development writes readable
//! lines to the console. Both carry request and operation identifiers, so a
//! user reporting "it said something went wrong" leads to an exact line.
//!
//! Redaction here is defence in depth, not the primary control. The primary
//! control is that secrets live in a wrapper type with no printing
//! implementation (Phase 3, `crates/security`). This layer catches the case
//! where a secret arrives as a plain string in a field name that gives it away.

use std::path::Path;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;

/// Field names whose values are never written to a log, whatever they contain.
pub const REDACTED_FIELDS: &[&str] = &[
    "password",
    "password_hash",
    "token",
    "session_token",
    "secret",
    "api_key",
    "authorization",
    "cookie",
    "set-cookie",
    "private_key",
    "encryption_key",
    "recovery_code",
    "value_cipher",
    "pairing_code",
];

pub const REDACTION_PLACEHOLDER: &str = "[redacted]";

/// True when a field name means the value must not be logged.
///
/// Matching is case-insensitive and substring-based, and separators are
/// normalised so `x-api-key`, `X_API_KEY` and `apiKey` are all caught — HTTP
/// headers use hyphens where Rust fields use underscores, and a matcher that
/// missed that would leak exactly the header carrying the credential.
///
/// Over-redacting a field is a cosmetic problem; under-redacting one is a
/// disclosed credential.
pub fn is_sensitive_field(name: &str) -> bool {
    let normalised: String = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .map(|character| character.to_ascii_lowercase())
        .collect();
    REDACTED_FIELDS.iter().any(|candidate| {
        let candidate: String = candidate
            .chars()
            .filter(char::is_ascii_alphanumeric)
            .collect();
        normalised.contains(&candidate)
    })
}

/// Redact by field name, for structures assembled before they reach `tracing`
/// — audit metadata in particular.
pub fn redact_pairs<I, K, V>(pairs: I) -> Vec<(String, String)>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    pairs
        .into_iter()
        .map(|(key, value)| {
            let key = key.as_ref().to_string();
            if is_sensitive_field(&key) {
                (key, REDACTION_PLACEHOLDER.to_string())
            } else {
                (key, value.as_ref().to_string())
            }
        })
        .collect()
}

/// Held for the lifetime of the process. Dropping it flushes buffered log
/// lines, so it must outlive everything that logs — losing the last lines
/// before a crash is exactly when they matter most.
#[derive(Debug)]
pub struct LoggingGuard {
    _file_guard: Option<WorkerGuard>,
}

/// Install the global subscriber.
///
/// Returns an error if one is already installed, which happens only if this is
/// called twice — a bug worth surfacing rather than ignoring.
pub fn init(config: &AppConfig, log_dir: &Path) -> Result<LoggingGuard, LoggingError> {
    let filter = EnvFilter::try_from_env("PROJECT_HOST_LOG")
        .unwrap_or_else(|_| EnvFilter::new(config.log_level.as_str()));

    if config.log_json {
        std::fs::create_dir_all(log_dir).map_err(|source| LoggingError::LogDirectory {
            path: log_dir.to_path_buf(),
            source,
        })?;

        let appender = tracing_appender::rolling::daily(log_dir, "project-host.log");
        let (writer, guard) = tracing_appender::non_blocking(appender);

        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .json()
                    .with_current_span(true)
                    .with_span_list(false)
                    .with_target(true)
                    .with_writer(writer),
            )
            .try_init()
            .map_err(|error| LoggingError::AlreadyInitialised(error.to_string()))?;

        Ok(LoggingGuard {
            _file_guard: Some(guard),
        })
    } else {
        // Development: a person is watching a console.
        tracing_subscriber::registry()
            .with(filter)
            .with(
                tracing_subscriber::fmt::layer()
                    .with_target(true)
                    .with_ansi(!config.mode.is_production()),
            )
            .try_init()
            .map_err(|error| LoggingError::AlreadyInitialised(error.to_string()))?;

        Ok(LoggingGuard { _file_guard: None })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoggingError {
    #[error("could not create the log directory {path}")]
    LogDirectory {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("a global logger is already installed: {0}")]
    AlreadyInitialised(String),
}

/// Correlation identifiers carried through a request's spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestContext {
    pub request_id: String,
    pub operation_id: Option<String>,
}

impl RequestContext {
    pub fn new(request_id: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            operation_id: None,
        }
    }

    pub fn with_operation(mut self, operation_id: impl Into<String>) -> Self {
        self.operation_id = Some(operation_id.into());
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LogLevel;

    #[test]
    fn obvious_secret_fields_are_recognised() {
        for field in [
            "password",
            "PASSWORD",
            "session_token",
            "discord_token",
            "Authorization",
            // HTTP headers use hyphens; camelCase appears in JSON payloads.
            "x-api-key",
            "X-API-KEY",
            "apiKey",
            "Set-Cookie",
            "encryption_key",
            "recovery_code",
        ] {
            assert!(is_sensitive_field(field), "{field} should be redacted");
        }
    }

    #[test]
    fn ordinary_fields_are_left_alone() {
        for field in ["project_id", "status", "port", "display_name", "created_at"] {
            assert!(!is_sensitive_field(field), "{field} should not be redacted");
        }
    }

    #[test]
    fn redaction_replaces_the_value_and_keeps_the_key() {
        // The key is kept deliberately: knowing a token was present is useful
        // when reading a log, and the key name itself is not the secret.
        let redacted = redact_pairs([
            ("project_id", "prj_123"),
            ("discord_token", "a-real-token-value"),
        ]);
        assert_eq!(
            redacted[0],
            ("project_id".to_string(), "prj_123".to_string())
        );
        assert_eq!(
            redacted[1],
            (
                "discord_token".to_string(),
                REDACTION_PLACEHOLDER.to_string()
            )
        );
    }

    #[test]
    fn no_secret_value_survives_redaction() {
        let secret = "super-secret-value";
        let redacted = redact_pairs([("api_key", secret), ("password", secret)]);
        for (_, value) in &redacted {
            assert_ne!(value, secret);
            assert_eq!(value, REDACTION_PLACEHOLDER);
        }
    }

    #[test]
    fn production_defaults_to_json_logging() {
        let config = AppConfig::default();
        assert!(config.mode.is_production());
        assert!(config.log_json, "production must write structured logs");
    }

    #[test]
    fn request_context_carries_both_identifiers() {
        let context = RequestContext::new("req_1").with_operation("op_2");
        assert_eq!(context.request_id, "req_1");
        assert_eq!(context.operation_id.as_deref(), Some("op_2"));
    }

    #[test]
    fn a_development_config_uses_console_logging() {
        let config = AppConfig {
            mode: crate::config::Mode::Development,
            log_json: false,
            log_level: LogLevel::Debug,
            ..AppConfig::default()
        };
        assert!(!config.log_json);
        assert_eq!(config.log_level.as_str(), "debug");
    }
}
