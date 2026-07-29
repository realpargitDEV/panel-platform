//! Application configuration.
//!
//! Parsed once, at startup. The process refuses to start when anything is
//! malformed rather than running half-configured and revealing the problem at
//! the first operation that depends on it.
//!
//! Precedence, lowest first: built-in defaults, then `config.toml`, then
//! `PROJECT_HOST_*` environment variables. Environment wins so a developer can
//! override one value without editing the file the installer owns.
//!
//! There are no network settings here. The application runs in one process on
//! the user's machine and listens on nothing; the bind address, port and LAN
//! consent flag that used to live here had no listener left to describe.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Development or production. Deciding this from an explicit value — never from
/// "is a debug build" — is what stops a release binary picking up development
/// defaults because someone set one stray variable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Development,
    #[default]
    Production,
}

impl Mode {
    pub fn is_production(self) -> bool {
        matches!(self, Mode::Production)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Error,
    Warn,
    Info,
    Debug,
    Trace,
}

impl LogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            LogLevel::Error => "error",
            LogLevel::Warn => "warn",
            LogLevel::Info => "info",
            LogLevel::Debug => "debug",
            LogLevel::Trace => "trace",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read {path}")]
    Read {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not parse {path}: {source}")]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("`{name}` is not a valid {expected}: {value}")]
    InvalidEnvironment {
        name: &'static str,
        expected: &'static str,
        value: String,
    },

    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AppConfig {
    pub mode: Mode,

    pub log_level: LogLevel,
    /// Structured JSON to a rotated file. Off only in development, where a
    /// human is reading the console.
    pub log_json: bool,
    pub log_retention_days: u16,

    /// Hard ceiling on projects, guarding against a runaway loop exhausting
    /// the host.
    pub max_projects: u32,
    pub max_upload_bytes: u64,
    pub max_archive_entries: u32,
    pub max_extracted_bytes: u64,

    /// Host ports handed out to projects automatically.
    pub port_pool_start: u16,
    pub port_pool_end: u16,

    pub docker_enabled: bool,

    /// Overrides for the platform defaults. Left empty in a normal install.
    pub data_dir: Option<PathBuf>,
    pub config_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
    pub projects_dir: Option<PathBuf>,
    pub backups_dir: Option<PathBuf>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            mode: Mode::Production,
            log_level: LogLevel::Info,
            log_json: true,
            log_retention_days: 14,
            max_projects: 50,
            max_upload_bytes: 2 * 1024 * 1024 * 1024,
            max_archive_entries: 50_000,
            max_extracted_bytes: 10 * 1024 * 1024 * 1024,
            port_pool_start: 20_000,
            port_pool_end: 29_999,
            docker_enabled: true,
            data_dir: None,
            config_dir: None,
            log_dir: None,
            projects_dir: None,
            backups_dir: None,
        }
    }
}

impl AppConfig {
    /// Read `config.toml` if present, apply environment overrides, validate.
    ///
    /// A missing file is not an error: every value has a working default, and
    /// an install that has not been customised should still start.
    pub fn load(config_path: &Path) -> Result<Self, ConfigError> {
        let mut config = match std::fs::read_to_string(config_path) {
            Ok(contents) => toml::from_str(&contents).map_err(|source| ConfigError::Parse {
                path: config_path.to_path_buf(),
                source,
            })?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
            Err(source) => {
                return Err(ConfigError::Read {
                    path: config_path.to_path_buf(),
                    source,
                })
            }
        };

        config.apply_environment(|name| std::env::var(name).ok())?;
        config.validate()?;
        Ok(config)
    }

    /// Apply `PROJECT_HOST_*` overrides. The lookup is injected so this is
    /// testable without mutating the process environment, which would make
    /// tests order-dependent.
    pub fn apply_environment<F>(&mut self, lookup: F) -> Result<(), ConfigError>
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(value) = lookup("PROJECT_HOST_MODE") {
            self.mode = match value.to_ascii_lowercase().as_str() {
                "development" | "dev" => Mode::Development,
                "production" | "prod" => Mode::Production,
                _ => {
                    return Err(ConfigError::InvalidEnvironment {
                        name: "PROJECT_HOST_MODE",
                        expected: "mode (development or production)",
                        value,
                    })
                }
            };
        }

        if let Some(value) = lookup("PROJECT_HOST_LOG_LEVEL") {
            self.log_level = match value.to_ascii_lowercase().as_str() {
                "error" => LogLevel::Error,
                "warn" => LogLevel::Warn,
                "info" => LogLevel::Info,
                "debug" => LogLevel::Debug,
                "trace" => LogLevel::Trace,
                _ => {
                    return Err(ConfigError::InvalidEnvironment {
                        name: "PROJECT_HOST_LOG_LEVEL",
                        expected: "log level",
                        value,
                    })
                }
            };
        }

        if let Some(value) = lookup("PROJECT_HOST_DOCKER_ENABLED") {
            self.docker_enabled = parse_bool(&value).ok_or(ConfigError::InvalidEnvironment {
                name: "PROJECT_HOST_DOCKER_ENABLED",
                expected: "boolean",
                value: value.clone(),
            })?;
        }

        if let Some(value) = lookup("PROJECT_HOST_DATA_DIR") {
            self.data_dir = Some(PathBuf::from(value));
        }

        Ok(())
    }

    /// Reject configurations that are internally inconsistent or unsafe.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.port_pool_start < 1024 {
            return Err(ConfigError::Invalid(
                "port_pool_start must be 1024 or above".to_string(),
            ));
        }
        if self.port_pool_start >= self.port_pool_end {
            return Err(ConfigError::Invalid(format!(
                "port pool {}–{} is empty",
                self.port_pool_start, self.port_pool_end
            )));
        }

        if self.max_projects == 0 {
            return Err(ConfigError::Invalid(
                "max_projects must be at least 1".to_string(),
            ));
        }
        if self.max_upload_bytes == 0 {
            return Err(ConfigError::Invalid(
                "max_upload_bytes must be non-zero".to_string(),
            ));
        }

        // Production must never quietly run with development logging: a trace
        // level in production writes operation detail to disk indefinitely.
        if self.mode.is_production() && matches!(self.log_level, LogLevel::Trace) {
            return Err(ConfigError::Invalid(
                "trace logging is not permitted in production; it records operation detail"
                    .to_string(),
            ));
        }

        Ok(())
    }

    /// How many ports the pool can hand out.
    pub fn port_pool_size(&self) -> u32 {
        u32::from(self.port_pool_end - self.port_pool_start) + 1
    }
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Some(true),
        "0" | "false" | "no" | "off" => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> = pairs
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect();
        move |name: &str| map.get(name).cloned()
    }

    #[test]
    fn defaults_are_production() {
        // A binary that has not been told otherwise must assume production.
        assert_eq!(AppConfig::default().mode, Mode::Production);
        assert!(AppConfig::default().log_json);
        assert!(AppConfig::default().validate().is_ok());
    }

    #[test]
    fn an_empty_port_pool_is_refused() {
        let config = AppConfig {
            port_pool_start: 30_000,
            port_pool_end: 30_000,
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn a_privileged_port_pool_is_refused() {
        let config = AppConfig {
            port_pool_start: 80,
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn trace_logging_is_refused_in_production() {
        let config = AppConfig {
            mode: Mode::Production,
            log_level: LogLevel::Trace,
            ..AppConfig::default()
        };
        assert!(config.validate().is_err());

        let development = AppConfig {
            mode: Mode::Development,
            log_level: LogLevel::Trace,
            ..AppConfig::default()
        };
        assert!(
            development.validate().is_ok(),
            "trace is fine in development"
        );
    }

    #[test]
    fn environment_overrides_are_applied() {
        let mut config = AppConfig::default();
        config
            .apply_environment(env(&[
                ("PROJECT_HOST_MODE", "development"),
                ("PROJECT_HOST_LOG_LEVEL", "debug"),
                ("PROJECT_HOST_DOCKER_ENABLED", "false"),
            ]))
            .expect("valid overrides");

        assert_eq!(config.mode, Mode::Development);
        assert_eq!(config.log_level, LogLevel::Debug);
        assert!(!config.docker_enabled);
    }

    #[test]
    fn a_malformed_environment_value_names_the_variable() {
        let mut config = AppConfig::default();
        let error = config
            .apply_environment(env(&[("PROJECT_HOST_DOCKER_ENABLED", "maybe")]))
            .expect_err("should fail");
        let message = error.to_string();
        assert!(
            message.contains("PROJECT_HOST_DOCKER_ENABLED"),
            "got {message}"
        );
        assert!(message.contains("maybe"), "got {message}");
    }

    #[test]
    fn an_unknown_mode_is_refused_rather_than_defaulted() {
        // Defaulting an unrecognised mode to production would be safe; to
        // development would not. Refusing avoids having to be right.
        let mut config = AppConfig::default();
        assert!(config
            .apply_environment(env(&[("PROJECT_HOST_MODE", "staging")]))
            .is_err());
    }

    #[test]
    fn booleans_accept_the_usual_spellings() {
        for truthy in ["1", "true", "TRUE", "yes", "on"] {
            let mut config = AppConfig {
                docker_enabled: false,
                ..AppConfig::default()
            };
            config
                .apply_environment(env(&[("PROJECT_HOST_DOCKER_ENABLED", truthy)]))
                .expect("valid");
            assert!(config.docker_enabled, "{truthy} should be true");
        }
        for falsy in ["0", "false", "no", "off"] {
            let mut config = AppConfig::default();
            config
                .apply_environment(env(&[("PROJECT_HOST_DOCKER_ENABLED", falsy)]))
                .expect("valid");
            assert!(!config.docker_enabled, "{falsy} should be false");
        }
    }

    #[test]
    fn missing_file_falls_back_to_defaults() {
        let directory = tempfile::tempdir().expect("temp dir");
        let config = AppConfig::load(&directory.path().join("absent.toml")).expect("load");
        assert_eq!(config.max_projects, AppConfig::default().max_projects);
    }

    #[test]
    fn a_toml_file_is_read() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("config.toml");
        std::fs::write(
            &path,
            "mode = \"development\"\nmax_projects = 12\nlog_level = \"debug\"\n",
        )
        .expect("write");

        let config = AppConfig::load(&path).expect("load");
        assert_eq!(config.mode, Mode::Development);
        assert_eq!(config.max_projects, 12);
        assert_eq!(config.log_level, LogLevel::Debug);
    }

    #[test]
    fn an_unknown_key_is_rejected_rather_than_ignored() {
        // A typo in a config file should be loud. Silently ignoring
        // `max_projectss = 100` would leave the user believing they had
        // changed something.
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "max_projectss = 100\n").expect("write");
        assert!(AppConfig::load(&path).is_err());
    }

    #[test]
    fn a_malformed_file_reports_its_path() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("config.toml");
        std::fs::write(&path, "max_projects = = 9\n").expect("write");
        let error = AppConfig::load(&path).expect_err("should fail");
        assert!(error.to_string().contains("config.toml"), "got {error}");
    }

    #[test]
    fn port_pool_size_counts_both_ends() {
        let config = AppConfig {
            port_pool_start: 20_000,
            port_pool_end: 20_009,
            ..AppConfig::default()
        };
        assert_eq!(config.port_pool_size(), 10);
    }

    #[test]
    fn config_round_trips_through_toml() {
        let config = AppConfig::default();
        let text = toml::to_string(&config).expect("serialise");
        let back: AppConfig = toml::from_str(&text).expect("deserialise");
        assert_eq!(back, config);
    }
}
