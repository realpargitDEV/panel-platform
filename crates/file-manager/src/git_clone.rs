//! Cloning a project from a git remote.
//!
//! In-process, through `gix`. The alternative — shelling out to `git` — was
//! rejected for two reasons. It would add a host prerequisite the product
//! otherwise does not have (nothing it installs runs on the host outside a
//! container), and `git clone` runs a repository's hooks; a `post-checkout` hook
//! in a repository a user was talked into cloning would execute as that user.
//! `gix` runs no hooks at all.
//!
//! Three further decisions are load-bearing:
//!
//! - **Isolated configuration.** The repository is opened with
//!   [`gix::open::Options::isolated`], so the host's git configuration and
//!   environment are ignored. Otherwise a `url.<base>.insteadOf` rule in the
//!   user's global config could rewrite the URL that
//!   [`crate::remote_url`] just validated into a different host or a different
//!   protocol, and no amount of validating the input string would help.
//! - **No submodules.** A submodule is a URL inside the repository being cloned,
//!   which is to say a fetch of an attacker-chosen remote that never passed
//!   validation.
//! - **Shallow.** One commit is what is needed to run a project. It also bounds
//!   the transfer.

use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::remote_url::{guard_host, HostResolver, RemoteUrl, UrlError};
use crate::zip_import::{ArchiveError, Staging};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CloneError {
    #[error(transparent)]
    Url(#[from] UrlError),
    /// A full commit id cannot be cloned directly: fetching an arbitrary object
    /// by id requires the server to allow it, and most do not. Named explicitly
    /// rather than failing deep inside the protocol with a confusing message.
    #[error("a commit id cannot be cloned directly; use a branch or tag name")]
    CommitIdNotSupported,
    #[error("`{0}` is not a valid branch or tag name")]
    InvalidRef(String),
    #[error("the clone exceeded the {limit} byte limit")]
    TooLarge { limit: u64 },
    #[error("the clone took longer than {seconds} seconds")]
    TimedOut { seconds: u64 },
    #[error("the remote is empty")]
    EmptyRemote,
    /// `{0}` is `gix`'s message. It can name the host and the ref; it cannot name
    /// the token, which travels as a credential on the connection and is never
    /// part of the URL `gix` is given to print.
    #[error("the clone failed: {0}")]
    Git(String),
    #[error("`{0}` is not a directory inside the repository")]
    NoSuchSubdirectory(String),
    #[error(transparent)]
    Archive(#[from] ArchiveError),
    #[error("{0}")]
    Io(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CloneLimits {
    /// Ceiling on the checked-out tree plus its `.git` directory.
    pub max_bytes: u64,
    pub timeout: Duration,
}

impl Default for CloneLimits {
    fn default() -> Self {
        Self {
            max_bytes: 2 * 1024 * 1024 * 1024,
            timeout: Duration::from_secs(600),
        }
    }
}

/// What a clone produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CloneReport {
    /// The commit that was actually checked out — the only honest answer to
    /// "what is running" when the ref was a moving branch.
    pub commit: String,
    /// The ref that was asked for, or `None` for the remote's default branch.
    pub requested_ref: Option<String>,
    pub bytes: u64,
}

/// Why a running clone should be stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overrun {
    Bytes,
    Time,
}

/// The budget a clone runs under.
///
/// Split out as a plain predicate so the limit logic is unit-tested without a
/// network, a repository, or a two-gigabyte fixture.
#[derive(Debug, Clone, Copy)]
pub struct Budget {
    pub max_bytes: u64,
    pub deadline: Instant,
}

impl Budget {
    pub fn exceeded(&self, bytes_so_far: u64, now: Instant) -> Option<Overrun> {
        if bytes_so_far > self.max_bytes {
            return Some(Overrun::Bytes);
        }
        if now > self.deadline {
            return Some(Overrun::Time);
        }
        None
    }
}

/// Total bytes of a directory tree, not following symbolic links.
///
/// `symlink_metadata` rather than `metadata`: following a link would count what
/// it points at, which is both wrong and a way for a repository to make its own
/// size look like whatever it wants.
pub fn directory_size(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut stack = vec![root.to_path_buf()];

    while let Some(current) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&current) else {
            continue;
        };
        for entry in entries.flatten() {
            let Ok(metadata) = entry.path().symlink_metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(entry.path());
            } else {
                total = total.saturating_add(metadata.len());
            }
        }
    }
    total
}

/// Is this ref a raw commit id rather than a name?
fn looks_like_commit_id(reference: &str) -> bool {
    let length = reference.len();
    (7..=40).contains(&length) && reference.chars().all(|c| c.is_ascii_hexdigit())
}

/// A `.git` directory is not project content and must not be promoted into a
/// project's tree by the subdirectory path either.
fn is_git_internal(relative: &Path) -> bool {
    relative
        .components()
        .any(|component| component.as_os_str() == ".git")
}

/// Refuse a tree containing a symbolic link that leaves it.
///
/// Git can carry symlinks, and a link to `/` or `C:\` inside a project would be
/// followed by anything that walks the tree without the care
/// [`crate::safe_path`] takes. Links that stay inside the tree are left alone:
/// they are ordinary in real repositories.
pub fn refuse_escaping_symlinks(root: &Path) -> Result<(), CloneError> {
    let canonical_root = dunce::canonicalize(root).map_err(|e| CloneError::Io(e.to_string()))?;
    let mut stack = vec![canonical_root.clone()];

    while let Some(current) = stack.pop() {
        let entries = std::fs::read_dir(&current).map_err(|e| CloneError::Io(e.to_string()))?;
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = path.symlink_metadata() else {
                continue;
            };

            if metadata.is_symlink() {
                // An unresolvable link points at nothing reachable, which is not
                // an escape; a resolvable one outside the tree is.
                if let Ok(target) = dunce::canonicalize(&path) {
                    if !target.starts_with(&canonical_root) {
                        return Err(CloneError::Archive(ArchiveError::ForbiddenEntryKind {
                            name: path.to_string_lossy().to_string(),
                            kind: "symbolic link leaving the repository",
                        }));
                    }
                }
                continue;
            }

            if metadata.is_dir() {
                stack.push(path);
            }
        }
    }
    Ok(())
}

/// Clone a validated remote into a directory that does not yet exist.
///
/// `token`, when present, authenticates over HTTPS. It is set on the URL handed
/// to the transport and nowhere else: it is not written to the repository's
/// config, so the cloned project does not carry the user's credential around in
/// `.git/config`.
fn clone_into(
    url: &RemoteUrl,
    git_ref: Option<&str>,
    token: Option<&str>,
    into: &Path,
    limits: &CloneLimits,
) -> Result<String, CloneError> {
    if let Some(reference) = git_ref {
        if looks_like_commit_id(reference) {
            return Err(CloneError::CommitIdNotSupported);
        }
    }

    let mut git_url =
        gix::url::parse(url.as_str().into()).map_err(|error| CloneError::Git(error.to_string()))?;
    if let Some(token) = token {
        // GitHub, GitLab and Bitbucket all accept a token as the password with
        // any username over HTTPS.
        git_url.set_user(Some("token".to_string()));
        git_url.set_password(Some(token.to_string()));
    }

    let mut prepare = gix::clone::PrepareFetch::new(
        git_url,
        into,
        gix::create::Kind::WithWorktree,
        gix::create::Options::default(),
        // Isolated: the host's global git configuration and git environment
        // variables are ignored, so no `insteadOf` rule can rewrite the URL that
        // was just validated.
        gix::open::Options::isolated(),
    )
    .map_err(|error| CloneError::Git(error.to_string()))?
    .with_shallow(gix::remote::fetch::Shallow::DepthAtRemote(
        NonZeroU32::new(1).unwrap_or(NonZeroU32::MIN),
    ));

    if let Some(reference) = git_ref {
        prepare = prepare
            .with_ref_name(Some(reference))
            .map_err(|_| CloneError::InvalidRef(reference.to_string()))?;
    }

    // The interrupt flag is how the byte and time limits are enforced *during*
    // the transfer rather than after it: a watcher thread measures the directory
    // as it grows and flips the flag, and gix unwinds.
    let interrupt = Arc::new(AtomicBool::new(false));
    let finished = Arc::new(AtomicBool::new(false));
    let budget = Budget {
        max_bytes: limits.max_bytes,
        deadline: Instant::now() + limits.timeout,
    };
    let overrun = Arc::new(std::sync::Mutex::new(None::<Overrun>));

    let watcher = {
        let interrupt = Arc::clone(&interrupt);
        let finished = Arc::clone(&finished);
        let overrun = Arc::clone(&overrun);
        let watched = into.to_path_buf();
        std::thread::spawn(move || {
            while !finished.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(200));
                if let Some(reason) = budget.exceeded(directory_size(&watched), Instant::now()) {
                    if let Ok(mut slot) = overrun.lock() {
                        *slot = Some(reason);
                    }
                    interrupt.store(true, Ordering::Relaxed);
                    return;
                }
            }
        })
    };

    let outcome = run_clone(&mut prepare, &interrupt);

    finished.store(true, Ordering::Relaxed);
    let _ = watcher.join();

    // A limit overrun surfaces from gix as a generic interruption, so the reason
    // recorded by the watcher is what the caller is told.
    if let Some(reason) = overrun.lock().ok().and_then(|slot| *slot) {
        return Err(match reason {
            Overrun::Bytes => CloneError::TooLarge {
                limit: limits.max_bytes,
            },
            Overrun::Time => CloneError::TimedOut {
                seconds: limits.timeout.as_secs(),
            },
        });
    }

    outcome
}

/// The gix calls themselves, kept apart so the limit plumbing above stays
/// readable.
fn run_clone(
    prepare: &mut gix::clone::PrepareFetch,
    interrupt: &AtomicBool,
) -> Result<String, CloneError> {
    let (mut checkout, _fetch) = prepare
        .fetch_then_checkout(gix::progress::Discard, interrupt)
        .map_err(|error| CloneError::Git(error.to_string()))?;

    let (repository, _outcome) = checkout
        .main_worktree(gix::progress::Discard, interrupt)
        .map_err(|error| CloneError::Git(error.to_string()))?;

    let head = repository
        .head_id()
        .map_err(|_| CloneError::EmptyRemote)?
        .to_string();
    Ok(head)
}

/// Everything a clone needs, grouped so a call site reads as a description of
/// the clone rather than nine positional arguments.
#[derive(Debug, Clone, Copy)]
pub struct CloneRequest<'a> {
    pub url: &'a str,
    /// Branch or tag. `None` means the remote's default branch.
    pub git_ref: Option<&'a str>,
    /// Path within the repository to promote, for repositories holding more than
    /// one project.
    pub subdirectory: Option<&'a str>,
    pub token: Option<&'a str>,
    pub staging_root: &'a Path,
    pub destination: &'a Path,
    /// Names the staging directory. Generated by the application, never taken
    /// from a caller.
    pub clone_id: &'a str,
    pub limits: CloneLimits,
}

/// Clone a remote into a new project directory.
///
/// The clone lands in a UUID-named staging directory and is renamed into place
/// only after it has been checked: within its byte budget, free of symbolic
/// links that leave it, and containing the subdirectory that was asked for. A
/// failure at any point leaves no project and no staging directory.
pub fn clone_project<R: HostResolver>(
    request: &CloneRequest<'_>,
    resolver: &R,
) -> Result<CloneReport, CloneError> {
    let CloneRequest {
        url: input,
        git_ref,
        subdirectory,
        token,
        staging_root,
        destination,
        clone_id,
        limits,
    } = *request;

    let url = RemoteUrl::parse(input)?;
    // Resolved and checked before a connection is opened. Note the honest
    // limitation: gix resolves the host again when it connects, so unlike the
    // archive path this cannot pin the address that was checked. A host that
    // answers differently between the two lookups is not excluded here.
    guard_host(&url, resolver)?;

    if destination.exists() {
        return Err(CloneError::Io("the destination already exists".to_string()));
    }

    let staging = Staging::new(staging_root, clone_id)?;
    let tree = staging.path().join("tree");

    let commit = clone_into(&url, git_ref, token, &tree, &limits)?;

    refuse_escaping_symlinks(&tree)?;

    let bytes = directory_size(&tree);
    if bytes > limits.max_bytes {
        return Err(CloneError::TooLarge {
            limit: limits.max_bytes,
        });
    }

    let promoted = match subdirectory {
        Some(relative) => {
            // Through `SafePath`, so `../..` or an absolute path is refused by
            // the same code that guards every other path in this crate.
            let safe = crate::safe_path::SafePath::new(&tree, relative)
                .map_err(|_| CloneError::NoSuchSubdirectory(relative.to_string()))?;
            if is_git_internal(Path::new(safe.relative())) {
                return Err(CloneError::NoSuchSubdirectory(relative.to_string()));
            }
            if !safe.absolute().is_dir() {
                return Err(CloneError::NoSuchSubdirectory(relative.to_string()));
            }
            safe.absolute().to_path_buf()
        }
        None => tree.clone(),
    };

    promote(staging, &promoted, destination)?;

    Ok(CloneReport {
        commit,
        requested_ref: git_ref.map(str::to_string),
        bytes,
    })
}

fn promote(staging: Staging, source: &Path, destination: &Path) -> Result<(), CloneError> {
    if let Some(parent) = destination.parent() {
        std::fs::create_dir_all(parent).map_err(|e| CloneError::Io(e.to_string()))?;
    }
    std::fs::rename(source, destination).map_err(|e| CloneError::Io(e.to_string()))?;
    // Whatever is left of the staging directory — the `.git` directory when a
    // subdirectory was promoted — goes with the drop.
    drop(staging);
    Ok(())
}

/// The path a clone would be staged at. Exposed for callers that report progress.
pub fn staging_path(staging_root: &Path, clone_id: &str) -> PathBuf {
    staging_root.join(format!("import-{clone_id}")).join("tree")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::IpAddr;
    use std::str::FromStr;

    struct AnyPublic;

    impl HostResolver for AnyPublic {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, UrlError> {
            Ok(vec![IpAddr::from_str("140.82.121.4").expect("address")])
        }
    }

    struct Loopback;

    impl HostResolver for Loopback {
        fn resolve(&self, _host: &str, _port: u16) -> Result<Vec<IpAddr>, UrlError> {
            Ok(vec![IpAddr::from_str("127.0.0.1").expect("address")])
        }
    }

    // ------------------------------------------------------------- budgets

    #[test]
    fn a_budget_within_both_limits_permits_the_clone() {
        let budget = Budget {
            max_bytes: 1000,
            deadline: Instant::now() + Duration::from_secs(60),
        };
        assert_eq!(budget.exceeded(999, Instant::now()), None);
    }

    #[test]
    fn a_budget_reports_which_limit_was_hit() {
        let now = Instant::now();
        let budget = Budget {
            max_bytes: 1000,
            deadline: now + Duration::from_secs(60),
        };
        assert_eq!(budget.exceeded(1001, now), Some(Overrun::Bytes));

        let expired = Budget {
            max_bytes: 1000,
            deadline: now - Duration::from_secs(1),
        };
        assert_eq!(expired.exceeded(0, now), Some(Overrun::Time));
    }

    #[test]
    fn the_byte_limit_is_reported_before_the_deadline_when_both_are_blown() {
        // Both are true; the size is the more useful thing to tell a user.
        let now = Instant::now();
        let budget = Budget {
            max_bytes: 10,
            deadline: now - Duration::from_secs(1),
        };
        assert_eq!(budget.exceeded(11, now), Some(Overrun::Bytes));
    }

    // -------------------------------------------------------- measurement

    #[test]
    fn a_directory_is_measured_including_its_subdirectories() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::write(dir.path().join("a.txt"), vec![b'a'; 100]).expect("write");
        std::fs::create_dir_all(dir.path().join("nested/deeper")).expect("dirs");
        std::fs::write(dir.path().join("nested/b.txt"), vec![b'b'; 50]).expect("write");
        std::fs::write(dir.path().join("nested/deeper/c.txt"), vec![b'c'; 25]).expect("write");

        assert_eq!(directory_size(dir.path()), 175);
    }

    #[test]
    fn measuring_an_absent_directory_is_zero_rather_than_an_error() {
        // The watcher thread measures a directory gix has not created yet.
        assert_eq!(directory_size(Path::new("no/such/path/anywhere")), 0);
    }

    // ---------------------------------------------------------------- refs

    #[test]
    fn commit_ids_are_recognised_and_refused() {
        for reference in [
            "0f5c1d0a",
            "0f5c1d0ab1c2d3e4f5061728394a5b6c7d8e9f01",
            "abcdef1",
        ] {
            assert!(
                looks_like_commit_id(reference),
                "{reference} should be recognised as a commit id"
            );
        }
    }

    #[test]
    fn branch_and_tag_names_are_not_mistaken_for_commit_ids() {
        for reference in ["main", "v1.2.3", "feat/one", "release", "abcdefg-1", "dev"] {
            assert!(
                !looks_like_commit_id(reference),
                "{reference} should be treated as a name"
            );
        }
    }

    #[test]
    fn a_commit_id_is_refused_before_any_connection_is_opened() {
        let dir = tempfile::tempdir().expect("temp dir");
        let url = RemoteUrl::parse("https://github.com/owner/repo.git").expect("url");
        let result = clone_into(
            &url,
            Some("0f5c1d0ab1c2d3e4f5061728394a5b6c7d8e9f01"),
            None,
            &dir.path().join("tree"),
            &CloneLimits::default(),
        );
        assert_eq!(result, Err(CloneError::CommitIdNotSupported));
    }

    // ----------------------------------------------------------- guarding

    /// A request with the fields these tests do not vary already filled in.
    fn request<'a>(
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

    #[test]
    fn a_remote_resolving_to_loopback_is_refused_without_cloning() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("projects/prj_1");
        let result = clone_project(
            &request(
                "https://looks-fine.example.com/owner/repo.git",
                dir.path(),
                &destination,
                "guard-test",
            ),
            &Loopback,
        );
        assert!(
            matches!(
                result,
                Err(CloneError::Url(UrlError::ForbiddenAddress { .. }))
            ),
            "got {result:?}"
        );
        assert!(!dir.path().join("import-guard-test").exists());
    }

    #[test]
    fn a_non_https_remote_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("projects/prj_x");
        for input in [
            "ssh://git@github.com/owner/repo.git",
            "git://github.com/owner/repo.git",
            "file:///C:/repos/local",
        ] {
            let result = clone_project(
                &request(input, dir.path(), &destination, "scheme-test"),
                &AnyPublic,
            );
            assert!(
                matches!(
                    result,
                    Err(CloneError::Url(
                        UrlError::NotHttps { .. } | UrlError::Malformed
                    ))
                ),
                "{input} produced {result:?}"
            );
        }
    }

    #[test]
    fn cloning_onto_an_existing_destination_is_refused() {
        let dir = tempfile::tempdir().expect("temp dir");
        let destination = dir.path().join("existing");
        std::fs::create_dir_all(&destination).expect("create");

        let result = clone_project(
            &request(
                "https://github.com/owner/repo.git",
                dir.path(),
                &destination,
                "exists-test",
            ),
            &AnyPublic,
        );
        assert!(matches!(result, Err(CloneError::Io(_))));
    }

    // -------------------------------------------------------------- trees

    #[test]
    fn a_tree_without_symlinks_is_accepted() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("src")).expect("dirs");
        std::fs::write(dir.path().join("src/main.rs"), b"fn main() {}").expect("write");
        refuse_escaping_symlinks(dir.path()).expect("accepted");
    }

    #[test]
    fn a_symlink_leaving_the_tree_is_refused() {
        let outside = tempfile::tempdir().expect("temp dir");
        let secret = outside.path().join("secret.txt");
        std::fs::write(&secret, b"credentials").expect("write");

        let tree = tempfile::tempdir().expect("temp dir");
        let link = tree.path().join("link.txt");

        // Creating a symbolic link on Windows needs a privilege an ordinary
        // session does not have. Where it cannot be created, the rule cannot be
        // exercised, and the test says so rather than passing vacuously.
        if !make_symlink(&secret, &link) {
            eprintln!("skipped: this session cannot create symbolic links");
            return;
        }

        let result = refuse_escaping_symlinks(tree.path());
        assert!(
            matches!(result, Err(CloneError::Archive(_))),
            "got {result:?}"
        );
    }

    #[test]
    fn a_symlink_staying_inside_the_tree_is_allowed() {
        // Ordinary in real repositories; refusing it would reject valid projects.
        let tree = tempfile::tempdir().expect("temp dir");
        let target = tree.path().join("real.txt");
        std::fs::write(&target, b"content").expect("write");
        let link = tree.path().join("alias.txt");

        if !make_symlink(&target, &link) {
            eprintln!("skipped: this session cannot create symbolic links");
            return;
        }

        refuse_escaping_symlinks(tree.path()).expect("an internal link is fine");
    }

    #[cfg(windows)]
    fn make_symlink(target: &Path, link: &Path) -> bool {
        std::os::windows::fs::symlink_file(target, link).is_ok()
    }

    #[cfg(unix)]
    fn make_symlink(target: &Path, link: &Path) -> bool {
        std::os::unix::fs::symlink(target, link).is_ok()
    }

    #[test]
    fn a_git_directory_is_not_a_promotable_subdirectory() {
        for relative in [".git", ".git/hooks", "sub/.git"] {
            assert!(
                is_git_internal(Path::new(relative)),
                "{relative} should be recognised as git's own"
            );
        }
        assert!(!is_git_internal(Path::new("src/gitignore-tools")));
    }
}
