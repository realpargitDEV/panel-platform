//! Using the GitHub CLI that is installed on this machine.
//!
//! The point of this module is that a user who is already logged in with `gh`
//! should not have to paste a token to clone their private repository. It asks
//! `gh` two things: are you logged in, and what is your token.
//!
//! ## What it does not do, and why
//!
//! It does not run `gh repo clone`. That command shells out to `git`, which runs
//! the repository's hooks — a `post-checkout` hook in a repository a user was
//! talked into cloning would execute as that user, on the host. So the token `gh`
//! provides is handed to [`crate::git_clone`], which fetches in-process and runs
//! no hooks, and the URL still goes through [`crate::remote_url`] first.
//!
//! The user asked for the GitHub CLI option and gets it: their existing `gh`
//! login is what authenticates the fetch. If literally invoking `gh repo clone`
//! is wanted instead, that is a one-function change here — and it comes with the
//! hook-execution risk attached.
//!
//! ## What is trusted
//!
//! `gh` is a program on the user's PATH, chosen by the user. Its output is parsed
//! but never executed, and the token it returns is treated as a secret from the
//! moment it is read: it is not logged, not put in a URL, and not returned by any
//! type that derives `Debug`.

use std::process::Command;

use crate::remote_url::{RemoteUrl, UrlError};

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum GitHubCliError {
    /// `gh` is not on the PATH. Worth distinguishing from every other failure:
    /// the fix is "install it or paste a token", not "try again".
    #[error("the GitHub CLI (`gh`) is not installed, or not on the PATH")]
    NotInstalled,
    #[error("`gh` is installed but no account is logged in; run `gh auth login`")]
    NotAuthenticated,
    #[error("`gh` returned no token")]
    NoToken,
    #[error("`{0}` is not an `owner/repo` name or a GitHub URL")]
    NotARepository(String),
    #[error("running `gh` failed: {0}")]
    Failed(String),
    #[error(transparent)]
    Url(#[from] UrlError),
}

/// A repository named the way `gh` names them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoName {
    pub owner: String,
    pub repo: String,
}

impl RepoName {
    /// The HTTPS clone URL, built rather than accepted from input.
    pub fn clone_url(&self) -> String {
        format!("https://github.com/{}/{}.git", self.owner, self.repo)
    }
}

/// Parse `owner/repo`, or a GitHub URL, into a repository name.
///
/// Accepting both is the whole convenience: `gh repo clone owner/repo` is what
/// people type, and a pasted browser URL is what they paste.
///
/// The owner and repo segments are validated against GitHub's own rules rather
/// than trusted, because they are about to become part of a URL. Anything with a
/// slash, a space, a colon or a `..` in it is refused — a permissive parser here
/// would let `../../evil` become part of a fetch target.
pub fn parse_repo(input: &str) -> Result<RepoName, GitHubCliError> {
    let trimmed = input.trim().trim_end_matches('/');
    let refused = || GitHubCliError::NotARepository(input.to_string());

    // A URL form: take its path and carry on with the same rules.
    let path = if trimmed.starts_with("https://") || trimmed.starts_with("http://") {
        let url = url::Url::parse(trimmed).map_err(|_| refused())?;
        if url.host_str() != Some("github.com") && url.host_str() != Some("www.github.com") {
            return Err(refused());
        }
        url.path().trim_matches('/').to_string()
    } else {
        trimmed.to_string()
    };

    let stripped = path.strip_suffix(".git").unwrap_or(&path);
    let mut parts = stripped.split('/');

    let owner = parts.next().unwrap_or_default();
    let repo = parts.next().unwrap_or_default();
    // Exactly two segments. A longer path is a URL to something inside a
    // repository — a file, a pull request — not the repository itself.
    if parts.next().is_some() {
        return Err(refused());
    }

    if !is_valid_segment(owner) || !is_valid_segment(repo) {
        return Err(refused());
    }

    Ok(RepoName {
        owner: owner.to_string(),
        repo: repo.to_string(),
    })
}

/// GitHub's own rule: letters, digits, hyphens, underscores and dots, and not
/// empty. Notably this refuses `.` and `..`.
fn is_valid_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment.len() <= 100
        && segment != "."
        && segment != ".."
        && segment.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '-' | '_' | '.')
        })
}

/// How the CLI is invoked. Injected so every path here is testable without `gh`
/// installed, and so a test can assert on the arguments it would have used.
pub trait CommandRunner {
    /// Run `gh` with these arguments. `Ok` carries stdout; a non-zero exit is an
    /// `Err` with stderr.
    fn run(&self, arguments: &[&str]) -> Result<String, GitHubCliError>;
}

/// The real one.
#[derive(Debug, Clone, Copy, Default)]
pub struct GhCommand;

impl CommandRunner for GhCommand {
    fn run(&self, arguments: &[&str]) -> Result<String, GitHubCliError> {
        let output = Command::new("gh")
            .args(arguments)
            .output()
            .map_err(|error| {
                if error.kind() == std::io::ErrorKind::NotFound {
                    GitHubCliError::NotInstalled
                } else {
                    GitHubCliError::Failed(error.to_string())
                }
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            // `gh auth token` says this when nothing is logged in, and the
            // distinction is what lets the interface give the right advice.
            if stderr.contains("not logged") || stderr.contains("authentication") {
                return Err(GitHubCliError::NotAuthenticated);
            }
            return Err(GitHubCliError::Failed(stderr));
        }

        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

/// Is `gh` installed at all?
///
/// `--version` rather than `auth status`, because this question is about the
/// program and the interface asks it before showing the option.
pub fn is_available<R: CommandRunner>(runner: &R) -> bool {
    runner.run(&["--version"]).is_ok()
}

/// Who `gh` is logged in as, if anyone.
///
/// Parsed from `gh auth status`, whose text names the account. A parse failure is
/// reported as "logged in, name unknown" rather than as an error: the token is
/// what matters and the name is only for display.
pub fn logged_in_user<R: CommandRunner>(runner: &R) -> Result<Option<String>, GitHubCliError> {
    let output = runner.run(&["auth", "status"])?;
    Ok(extract_account(&output))
}

/// Pull the account name out of `gh auth status` output.
fn extract_account(output: &str) -> Option<String> {
    output
        .lines()
        .find_map(|line| line.split_once("account "))
        .map(|(_, rest)| rest)
        .and_then(|rest| rest.split_whitespace().next())
        .map(str::to_string)
}

/// The token `gh` holds for github.com.
///
/// The returned string is a credential. It goes straight to the fetch and is
/// never logged, never placed in a URL, and never stored by this function.
pub fn auth_token<R: CommandRunner>(runner: &R) -> Result<String, GitHubCliError> {
    let token = runner.run(&["auth", "token"])?;
    if token.is_empty() {
        return Err(GitHubCliError::NoToken);
    }
    Ok(token)
}

/// What the caller needs to clone a repository through the user's `gh` login.
#[derive(Clone, PartialEq, Eq)]
pub struct GhClone {
    pub url: RemoteUrl,
    pub token: String,
}

/// Deliberately redacted: this type exists to carry a token.
impl std::fmt::Debug for GhClone {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GhClone")
            .field("url", &self.url.as_str())
            .field("token", &"<redacted>")
            .finish()
    }
}

/// Resolve `owner/repo` into a validated URL plus the user's `gh` token.
///
/// The URL is *built* from the parsed name and then validated like any other, so
/// this path has no shortcut around [`crate::remote_url`]: a `gh`-sourced URL is
/// checked exactly as strictly as one a user typed.
pub fn prepare_clone<R: CommandRunner>(input: &str, runner: &R) -> Result<GhClone, GitHubCliError> {
    let name = parse_repo(input)?;
    let token = auth_token(runner)?;
    let url = RemoteUrl::parse(&name.clone_url())?;
    Ok(GhClone { url, token })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Records what it was asked and replays scripted answers.
    struct FakeGh {
        answers: Vec<(&'static str, Result<String, GitHubCliError>)>,
        seen: RefCell<Vec<String>>,
    }

    impl FakeGh {
        fn new(answers: Vec<(&'static str, Result<String, GitHubCliError>)>) -> Self {
            Self {
                answers,
                seen: RefCell::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.seen.borrow().clone()
        }
    }

    impl CommandRunner for FakeGh {
        fn run(&self, arguments: &[&str]) -> Result<String, GitHubCliError> {
            let joined = arguments.join(" ");
            self.seen.borrow_mut().push(joined.clone());

            self.answers
                .iter()
                .find(|(pattern, _)| joined.starts_with(pattern))
                .map(|(_, answer)| answer.clone())
                .unwrap_or(Err(GitHubCliError::Failed(format!("unscripted: {joined}"))))
        }
    }

    fn logged_in() -> FakeGh {
        FakeGh::new(vec![
            ("--version", Ok("gh version 2.63.2".to_string())),
            (
                "auth status",
                Ok("github.com\n  ✓ Logged in to github.com account octocat (keyring)".to_string()),
            ),
            ("auth token", Ok("gho_realtokenvalue".to_string())),
        ])
    }

    // ------------------------------------------------------------- parsing

    #[test]
    fn owner_slash_repo_is_accepted() {
        let name = parse_repo("cli/cli").expect("accepted");
        assert_eq!(name.owner, "cli");
        assert_eq!(name.repo, "cli");
        assert_eq!(name.clone_url(), "https://github.com/cli/cli.git");
    }

    #[test]
    fn a_pasted_github_url_is_accepted() {
        // What people actually paste.
        for input in [
            "https://github.com/cli/cli",
            "https://github.com/cli/cli.git",
            "https://github.com/cli/cli/",
            "  https://www.github.com/cli/cli  ",
        ] {
            let name = parse_repo(input).unwrap_or_else(|error| panic!("{input}: {error}"));
            assert_eq!(
                name.clone_url(),
                "https://github.com/cli/cli.git",
                "{input}"
            );
        }
    }

    #[test]
    fn a_url_to_something_inside_a_repository_is_refused() {
        // A pull request or a file is not a repository, and silently cloning the
        // repository it belongs to would be a guess.
        for input in [
            "https://github.com/cli/cli/pull/1234",
            "https://github.com/cli/cli/blob/main/README.md",
            "cli/cli/extra",
        ] {
            assert!(parse_repo(input).is_err(), "{input} should be refused");
        }
    }

    #[test]
    fn a_url_on_another_host_is_refused() {
        // This option is specifically the GitHub CLI's. A GitLab URL here would
        // be built into a github.com address, which is worse than refusing.
        for input in [
            "https://gitlab.com/owner/repo",
            "https://github.com.evil.example/owner/repo",
        ] {
            assert!(parse_repo(input).is_err(), "{input} should be refused");
        }
    }

    #[test]
    fn traversal_and_shell_characters_never_reach_a_url() {
        for input in [
            "../../evil/repo",
            "owner/../../../etc/passwd",
            "./x",
            "owner/repo;rm -rf /",
            "owner /repo",
            "owner/repo?query=1",
            "/repo",
            "owner/",
            "",
            "   ",
        ] {
            assert!(
                parse_repo(input).is_err(),
                "{input:?} should be refused, not turned into a URL"
            );
        }
    }

    #[test]
    fn dots_are_allowed_inside_a_name_but_not_alone() {
        // `owner/repo.js` is an ordinary repository name.
        assert!(parse_repo("owner/repo.js").is_ok());
        assert!(parse_repo("owner/.").is_err());
        assert!(parse_repo("owner/..").is_err());
    }

    // ------------------------------------------------------------ the CLI

    #[test]
    fn availability_is_asked_with_version() {
        let gh = logged_in();
        assert!(is_available(&gh));
        assert_eq!(gh.calls(), vec!["--version"]);
    }

    #[test]
    fn a_missing_gh_is_reported_as_missing_rather_than_as_a_failure() {
        // The fix is "install it or paste a token", which is different advice
        // from "try again".
        //
        // Every argument list gets the same answer, because that is how the real
        // thing behaves: `Command::new("gh")` fails to spawn regardless of what
        // was going to be passed to it.
        let gh = FakeGh::new(vec![
            ("--version", Err(GitHubCliError::NotInstalled)),
            ("auth", Err(GitHubCliError::NotInstalled)),
        ]);
        assert!(!is_available(&gh));
        assert_eq!(auth_token(&gh), Err(GitHubCliError::NotInstalled));
        assert_eq!(logged_in_user(&gh), Err(GitHubCliError::NotInstalled));
    }

    #[test]
    fn the_logged_in_account_is_read_from_auth_status() {
        assert_eq!(
            logged_in_user(&logged_in()).expect("status"),
            Some("octocat".to_string())
        );
    }

    #[test]
    fn being_logged_out_is_its_own_error() {
        let gh = FakeGh::new(vec![
            ("--version", Ok("gh version 2.63.2".to_string())),
            ("auth status", Err(GitHubCliError::NotAuthenticated)),
            ("auth token", Err(GitHubCliError::NotAuthenticated)),
        ]);
        assert_eq!(auth_token(&gh), Err(GitHubCliError::NotAuthenticated));
    }

    #[test]
    fn an_empty_token_is_not_treated_as_a_token() {
        let gh = FakeGh::new(vec![("auth token", Ok(String::new()))]);
        assert_eq!(auth_token(&gh), Err(GitHubCliError::NoToken));
    }

    // ------------------------------------------------------------ clone prep

    #[test]
    fn a_prepared_clone_carries_a_built_url_and_the_users_token() {
        let clone = prepare_clone("cli/cli", &logged_in()).expect("prepared");
        assert_eq!(clone.url.as_str(), "https://github.com/cli/cli.git");
        assert_eq!(clone.token, "gho_realtokenvalue");
    }

    #[test]
    fn a_prepared_clone_does_not_print_its_token() {
        // The realistic leak: a handler logging the value it is about to use.
        let clone = prepare_clone("cli/cli", &logged_in()).expect("prepared");
        let rendered = format!("{clone:?}");
        assert!(!rendered.contains("gho_realtokenvalue"), "{rendered}");
        assert!(rendered.contains("github.com/cli/cli"), "{rendered}");
    }

    #[test]
    fn a_bad_repository_name_is_refused_before_gh_is_asked_for_a_token() {
        // No reason to reach for a credential on behalf of a request that is
        // already invalid.
        let gh = logged_in();
        assert!(prepare_clone("../../evil", &gh).is_err());
        assert!(
            gh.calls().is_empty(),
            "gh was invoked for an invalid name: {:?}",
            gh.calls()
        );
    }
}
