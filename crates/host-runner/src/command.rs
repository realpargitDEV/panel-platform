//! Turning a project's runtime spec into a command this machine can run.
//!
//! Pure on purpose. Nothing here spawns anything, which is what makes every
//! runtime's translation testable on a machine that has none of them installed
//! — and this is the layer where the mistakes are, not in the spawning.
//!
//! The trap it exists to prevent: `RuntimeSpec::working_dir` defaults to
//! `/app`. That is a path inside a container image, and on the host it does not
//! exist. A host command must run in the project's own directory, and
//! [`start_command`] is the only place that decides so.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::probe::Toolchain;

/// A command ready to be spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessCommand {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum CommandError {
    #[error("{runtime} is not installed on this machine; looked for {}", looked_for.join(", "))]
    ToolchainMissing {
        runtime: String,
        looked_for: Vec<String>,
    },
    #[error("the project has no start command")]
    NoStartCommand,
    #[error("the start command `{0}` could not be read as a command")]
    Unparsable(String),
}

/// Everything needed to build one command.
#[derive(Debug, Clone)]
pub struct CommandInputs<'a> {
    pub runtime: &'a str,
    /// The spec's command, as the user or the planner wrote it.
    pub command: &'a str,
    /// Where the project's files actually are on this machine.
    pub project_directory: &'a Path,
    pub toolchain: &'a Toolchain,
    pub env: BTreeMap<String, String>,
    /// The host port allocated to this project, passed through the
    /// environment. There is no port mapping without a container: the process
    /// binds the real port itself, so it has to be told which one.
    pub port: Option<u16>,
}

/// Build the command that starts a project.
pub fn start_command(inputs: CommandInputs<'_>) -> Result<ProcessCommand, CommandError> {
    if let Toolchain::Missing { looked_for } = inputs.toolchain {
        return Err(CommandError::ToolchainMissing {
            runtime: inputs.runtime.to_string(),
            looked_for: looked_for.clone(),
        });
    }

    let mut words = split_command(inputs.command)?;
    if words.is_empty() {
        return Err(CommandError::NoStartCommand);
    }
    let program = words.remove(0);

    let mut env = inputs.env;
    if let Some(port) = inputs.port {
        env.insert("PORT".to_string(), port.to_string());
    }

    Ok(ProcessCommand {
        program,
        args: words,
        // Never `working_dir`: that is `/app`, a path inside an image.
        cwd: inputs.project_directory.to_path_buf(),
        env,
    })
}

/// Split a command line into words, respecting single and double quotes.
///
/// Not a shell. There is deliberately no expansion, no globbing, no pipes and
/// no redirection: a start command that needs those is asking for a shell, and
/// handing one an unquoted project path is how a directory with a space in it
/// becomes two arguments — or worse.
fn split_command(command: &str) -> Result<Vec<String>, CommandError> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut started = false;
    let mut quote: Option<char> = None;

    for character in command.chars() {
        match quote {
            Some(open) if character == open => quote = None,
            Some(_) => current.push(character),
            None if character == '\'' || character == '"' => {
                quote = Some(character);
                // A quoted empty string is still an argument.
                started = true;
            }
            None if character.is_whitespace() => {
                if started {
                    words.push(std::mem::take(&mut current));
                    started = false;
                }
            }
            None => {
                current.push(character);
                started = true;
            }
        }
    }

    if quote.is_some() {
        return Err(CommandError::Unparsable(command.to_string()));
    }
    if started {
        words.push(current);
    }

    Ok(words)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;

    fn found() -> Toolchain {
        Toolchain::Found {
            executable: PathBuf::from("/usr/bin/node"),
            version: Some("v22.3.0".to_string()),
        }
    }

    fn inputs<'a>(
        runtime: &'a str,
        command: &'a str,
        toolchain: &'a Toolchain,
    ) -> CommandInputs<'a> {
        CommandInputs {
            runtime,
            command,
            project_directory: Path::new("/projects/demo"),
            toolchain,
            env: BTreeMap::new(),
            port: None,
        }
    }

    /// The whole reason this module is pure. `working_dir` is `/app` in every
    /// runtime spec the planner writes, and on the host that directory does not
    /// exist — a command that ran there would fail with a path error that says
    /// nothing about the real problem.
    #[test]
    fn the_container_working_directory_never_reaches_a_host_command() {
        let toolchain = found();
        let command = start_command(inputs("NODEJS", "npm start", &toolchain)).expect("command");

        assert_eq!(command.cwd, PathBuf::from("/projects/demo"));
        assert!(
            !command.cwd.to_string_lossy().contains("app"),
            "the container path leaked into the host command: {:?}",
            command.cwd
        );
    }

    #[test]
    fn a_command_is_split_into_a_program_and_its_arguments() {
        let toolchain = found();
        let command =
            start_command(inputs("NODEJS", "node server.js --port 3000", &toolchain)).expect("c");

        assert_eq!(command.program, "node");
        assert_eq!(command.args, vec!["server.js", "--port", "3000"]);
    }

    /// Without a container there is no port mapping, so the process has to be
    /// told which port it was given.
    #[test]
    fn the_allocated_port_is_passed_through_the_environment() {
        let toolchain = found();
        let command = start_command(CommandInputs {
            port: Some(8081),
            ..inputs("NODEJS", "npm start", &toolchain)
        })
        .expect("command");

        assert_eq!(command.env.get("PORT").map(String::as_str), Some("8081"));
    }

    #[test]
    fn the_projects_own_environment_is_kept_and_port_does_not_erase_it() {
        let toolchain = found();
        let mut env = BTreeMap::new();
        env.insert("TOKEN".to_string(), "secret".to_string());

        let command = start_command(CommandInputs {
            env,
            port: Some(9000),
            ..inputs("NODEJS", "npm start", &toolchain)
        })
        .expect("command");

        assert_eq!(command.env.get("TOKEN").map(String::as_str), Some("secret"));
        assert_eq!(command.env.get("PORT").map(String::as_str), Some("9000"));
    }

    /// A directory with a space in it is the ordinary case on Windows —
    /// `C:\Users\Someone\My Projects`. Splitting on whitespace alone turns one
    /// argument into two and the project starts against the wrong path.
    #[test]
    fn quoted_arguments_survive_splitting() {
        let toolchain = found();
        let command = start_command(inputs(
            "PYTHON",
            "python -m http.server --directory \"My Projects/site\"",
            &toolchain,
        ))
        .expect("command");

        assert_eq!(
            command.args.last().map(String::as_str),
            Some("My Projects/site")
        );
        assert_eq!(command.args.len(), 4);
    }

    #[test]
    fn single_quotes_work_the_same_way() {
        let toolchain = found();
        let command = start_command(inputs("NODEJS", "node -e 'console.log(1)'", &toolchain))
            .expect("command");

        assert_eq!(command.args, vec!["-e", "console.log(1)"]);
    }

    #[test]
    fn an_unclosed_quote_is_refused_rather_than_guessed_at() {
        let toolchain = found();
        assert!(matches!(
            start_command(inputs("NODEJS", "node \"server.js", &toolchain)),
            Err(CommandError::Unparsable(_))
        ));
    }

    /// The refusal names the runtime and what was looked for, because the user
    /// has to know what to install.
    #[test]
    fn a_missing_toolchain_refuses_to_build_a_command() {
        let missing = Toolchain::Missing {
            looked_for: vec!["go".to_string()],
        };

        match start_command(inputs("GO", "go run .", &missing)) {
            Err(CommandError::ToolchainMissing {
                runtime,
                looked_for,
            }) => {
                assert_eq!(runtime, "GO");
                assert_eq!(looked_for, vec!["go"]);
            }
            other => panic!("expected ToolchainMissing, got {other:?}"),
        }
    }

    #[test]
    fn the_refusal_reads_as_a_sentence() {
        let missing = Toolchain::Missing {
            looked_for: vec!["python3".to_string(), "python".to_string()],
        };
        let error = start_command(inputs("PYTHON", "python app.py", &missing))
            .expect_err("must be refused");

        assert_eq!(
            error.to_string(),
            "PYTHON is not installed on this machine; looked for python3, python"
        );
    }

    #[test]
    fn an_empty_start_command_is_refused() {
        let toolchain = found();
        assert_eq!(
            start_command(inputs("NODEJS", "   ", &toolchain)),
            Err(CommandError::NoStartCommand)
        );
    }

    #[test]
    fn extra_whitespace_between_arguments_is_not_an_argument() {
        let toolchain = found();
        let command =
            start_command(inputs("NODEJS", "  node    server.js  ", &toolchain)).expect("c");

        assert_eq!(command.program, "node");
        assert_eq!(command.args, vec!["server.js"]);
    }
}
