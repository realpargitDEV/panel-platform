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

    /// A process tree outlived every attempt to end it. Reported rather than
    /// ignored because the consequence is a held port and a next start that
    /// fails for a reason nothing else explains.
    #[error("could not end the process tree rooted at {pid}")]
    ProcessSurvived { pid: u32 },
}
