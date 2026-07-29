//! Errors from the platform layer.

use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum PlatformError {
    #[error("could not create {path}")]
    Directory {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not set permissions on {path}")]
    Permissions {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("required environment variable `{name}` is not set")]
    MissingEnvironment { name: &'static str },

    #[error("this platform is not supported")]
    UnsupportedPlatform,
}
