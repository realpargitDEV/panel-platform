//! End-to-end checks against real remotes.
//!
//! `#[ignore]`d, so `cargo test` does not depend on the network. Run them
//! deliberately:
//!
//! ```text
//! cargo test -p project-host-file-manager --test remote_sources_network -- --ignored
//! ```
//!
//! Everything these cover that can be checked without a network already is,
//! next to the code: URL and address rules, redirect handling, entry rules,
//! byte budgets, staging cleanup. What is left, and what these are for, is the
//! part no fake can answer — that `gix` really clones over HTTPS and that a real
//! server's archive really extracts.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;

use project_host_file_manager::git_clone::{clone_project, CloneLimits, CloneRequest};
use project_host_file_manager::http_archive::{
    import_remote_archive, FetchLimits, RemoteArchiveRequest, ReqwestTransport,
};
use project_host_file_manager::remote_url::SystemResolver;
use project_host_file_manager::zip_import::ArchiveLimits;

/// A small, stable, public repository.
const REPO: &str = "https://github.com/octocat/Hello-World.git";
/// The same repository as an archive, served by GitHub's codeload host — which
/// also exercises the redirect path against a real server.
const ARCHIVE: &str = "https://github.com/octocat/Hello-World/archive/refs/heads/master.zip";

fn staging() -> tempfile::TempDir {
    tempfile::tempdir().expect("temp dir")
}

/// A clone request with the common fields filled in.
fn clone_request<'a>(
    url: &'a str,
    staging_root: &'a Path,
    destination: &'a Path,
    clone_id: &'a str,
) -> CloneRequest<'a> {
    CloneRequest {
        url,
        git_ref: None,
        subdirectory: None,
        token: None,
        staging_root,
        destination,
        clone_id,
        limits: CloneLimits::default(),
    }
}

fn assert_no_staging_left(root: &Path, id: &str) {
    assert!(
        !root.join(format!("import-{id}")).exists(),
        "the staging directory outlived the operation"
    );
}

#[test]
#[ignore = "needs the network"]
fn a_real_repository_clones_and_promotes() {
    let root = staging();
    let destination = root.path().join("projects/prj_clone");

    let report = clone_project(
        &clone_request(REPO, root.path(), &destination, "net-clone"),
        &SystemResolver,
    )
    .expect("the clone should succeed");

    assert_eq!(
        report.commit.len(),
        40,
        "a resolved commit id should be recorded: {}",
        report.commit
    );
    assert!(report.bytes > 0);
    assert!(
        destination.join("README").is_file(),
        "the working tree should have been checked out"
    );
    assert!(
        destination.join(".git").is_dir(),
        ".git is kept so the project can be updated from its remote later"
    );
    assert_no_staging_left(root.path(), "net-clone");
}

#[test]
#[ignore = "needs the network"]
fn a_named_ref_is_checked_out() {
    let root = staging();
    let destination = root.path().join("projects/prj_ref");

    let report = clone_project(
        &CloneRequest {
            git_ref: Some("master"),
            ..clone_request(REPO, root.path(), &destination, "net-ref")
        },
        &SystemResolver,
    )
    .expect("cloning a named branch should succeed");

    assert_eq!(report.requested_ref.as_deref(), Some("master"));
    assert!(destination.join("README").is_file());
}

#[test]
#[ignore = "needs the network"]
fn a_ref_that_does_not_exist_fails_without_leaving_a_project() {
    let root = staging();
    let destination = root.path().join("projects/prj_missing");

    let result = clone_project(
        &CloneRequest {
            git_ref: Some("no-such-branch-anywhere"),
            ..clone_request(REPO, root.path(), &destination, "net-missing")
        },
        &SystemResolver,
    );

    assert!(result.is_err(), "got {result:?}");
    assert!(
        !destination.exists(),
        "a failed clone left a project behind"
    );
    assert_no_staging_left(root.path(), "net-missing");
}

#[test]
#[ignore = "needs the network"]
fn a_byte_budget_smaller_than_the_repository_stops_the_clone() {
    let root = staging();
    let destination = root.path().join("projects/prj_tiny");

    let result = clone_project(
        &CloneRequest {
            limits: CloneLimits {
                max_bytes: 1024,
                ..CloneLimits::default()
            },
            ..clone_request(REPO, root.path(), &destination, "net-tiny")
        },
        &SystemResolver,
    );

    assert!(result.is_err(), "got {result:?}");
    assert!(!destination.exists());
    assert_no_staging_left(root.path(), "net-tiny");
}

#[test]
#[ignore = "needs the network"]
fn a_real_archive_url_downloads_and_extracts() {
    let root = staging();
    let destination = root.path().join("projects/prj_archive");

    let report = import_remote_archive(
        &RemoteArchiveRequest {
            url: ARCHIVE,
            token: None,
            staging_root: root.path(),
            destination: &destination,
            import_id: "net-archive",
            fetch_limits: FetchLimits::default(),
            archive_limits: ArchiveLimits::default(),
        },
        &ReqwestTransport,
        &SystemResolver,
    )
    .expect("the archive should download and extract");

    assert!(report.files > 0, "nothing was extracted");
    assert!(destination.is_dir());
    assert_no_staging_left(root.path(), "net-archive");
}

#[test]
#[ignore = "needs the network"]
fn a_subdirectory_can_be_promoted_on_its_own() {
    // The monorepo case: one repository, one project inside it.
    let root = staging();
    let destination = root.path().join("projects/prj_sub");

    let result = clone_project(
        &CloneRequest {
            subdirectory: Some("../outside"),
            ..clone_request(REPO, root.path(), &destination, "net-sub")
        },
        &SystemResolver,
    );

    // Traversal in the subdirectory is refused by `SafePath`, not by a check
    // written here — which is the point of routing it through that type.
    assert!(result.is_err(), "traversal should be refused: {result:?}");
    assert!(!destination.exists());
}
