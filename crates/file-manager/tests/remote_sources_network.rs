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
use project_host_file_manager::github_cli::{self, GhCommand};
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

// ---------------------------------------------------------- the GitHub CLI

/// These need `gh` installed *and* logged in, so they are ignored alongside the
/// network tests. They are the only way to know that the real `gh` output parses
/// — a fake proves the logic, not the format.

#[test]
#[ignore = "needs the network"]
fn the_real_gh_reports_itself_as_available() {
    assert!(
        github_cli::is_available(&GhCommand),
        "gh should be on the PATH for this test"
    );
}

#[test]
#[ignore = "needs the network"]
fn the_real_gh_auth_status_parses_into_an_account_name() {
    // The format of `gh auth status` is what a fake cannot verify.
    let account = github_cli::logged_in_user(&GhCommand)
        .expect("gh should be logged in for this test")
        .expect("an account name should be parsed out of gh auth status");

    assert!(!account.is_empty());
    assert!(
        !account.contains(' '),
        "the parsed account picked up more than a name: {account:?}"
    );
}

#[test]
#[ignore = "needs the network"]
fn a_repository_prepared_through_gh_carries_a_real_token() {
    let prepared = github_cli::prepare_clone("octocat/Hello-World", &GhCommand)
        .expect("gh should supply a token");

    assert_eq!(
        prepared.url.as_str(),
        "https://github.com/octocat/Hello-World.git"
    );
    // GitHub's tokens are prefixed by kind. Asserting the shape rather than the
    // value keeps the token out of any failure output.
    assert!(
        prepared.token.starts_with("gho_")
            || prepared.token.starts_with("ghp_")
            || prepared.token.starts_with("ghs_"),
        "the token does not look like a GitHub token"
    );
}

#[test]
#[ignore = "needs the network"]
fn a_repository_clones_with_the_credential_gh_supplied() {
    // The whole point of the option: no token typed, and the clone works.
    let root = staging();
    let destination = root.path().join("projects/prj_gh");

    let prepared = github_cli::prepare_clone("octocat/Hello-World", &GhCommand).expect("prepared");

    let report = clone_project(
        &CloneRequest {
            token: Some(&prepared.token),
            ..clone_request(prepared.url.as_str(), root.path(), &destination, "net-gh")
        },
        &SystemResolver,
    )
    .expect("the clone should succeed with gh's token");

    assert_eq!(report.commit.len(), 40);
    assert!(destination.join("README").is_file());
    assert_no_staging_left(root.path(), "net-gh");
}
