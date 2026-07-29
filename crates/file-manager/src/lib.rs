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
pub mod operations;
pub mod safe_path;
pub mod zip_import;

pub use extract::{extract_into, import_archive_file};
pub use operations::{EntryKind, FileEntry, FileError, FileLimits, Listing, TextFile};
pub use safe_path::{PathError, SafePath};
pub use zip_import::{ArchiveError, ArchiveLimits, ImportReport, Staging};
