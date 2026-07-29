//! Sandboxed filesystem access.
//!
//! The rule this crate exists to enforce: no `&str` from a request ever reaches
//! `std::fs`. Every operation takes a [`SafePath`], and the only way to build
//! one is through validation against a project root.
//!
//! Phase 5 delivers path safety and archive import — both fully verifiable
//! without Docker, and both named in the acceptance criteria.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod extract;
pub mod git_clone;
pub mod github_cli;
pub mod http_archive;
pub mod operations;
pub mod remote_url;
pub mod safe_path;
pub mod zip_import;

pub use extract::{extract_into, extract_tar_gzip_into, import_archive_file};
pub use git_clone::{clone_project, CloneError, CloneLimits, CloneReport, CloneRequest};
pub use github_cli::{is_available as gh_available, GhCommand, GitHubCliError, RepoName};
pub use http_archive::{
    import_remote_archive, ArchiveFormat, FetchError, FetchLimits, HttpTransport,
    RemoteArchiveRequest, ReqwestTransport,
};
pub use operations::{EntryKind, FileEntry, FileError, FileLimits, Listing, TextFile};
pub use remote_url::{HostResolver, RemoteUrl, SystemResolver, UrlError};
pub use safe_path::{PathError, SafePath};
pub use zip_import::{ArchiveError, ArchiveLimits, ImportReport, Staging};
