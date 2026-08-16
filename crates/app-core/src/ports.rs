//! Port conflicts, and who to blame for them.
//!
//! A container publishes a port through a mapping the daemon owns; a local
//! process binds the real port itself. So running many projects locally makes
//! port conflicts a first-class failure rather than an edge case, and the
//! difference between a good product and a bad one here is entirely in the
//! message.
//!
//! Three rules, all of them from the request and all of them load-bearing:
//!
//! * **Detect before spawning.** A project started against a taken port dies
//!   with `EADDRINUSE` a second later, and the reason is buried in its output.
//!   Asking first turns that into a refusal that names the port.
//! * **Name the holder.** "Port 20001 is in use" is something the user already
//!   knew. "Port 20001 is used by *Website*, which is running" is something
//!   they can act on.
//! * **Never take the port.** Killing whatever holds a port to free it for the
//!   project being started would mean one Start silently stopping another
//!   project — or an unrelated program the user was using.
//!
//! Changing the port is offered, never performed: [`suggest_free`] finds one
//! and [`assign`] applies it, and both are things a person asks for. A project
//! whose port is written in its own configuration file would break if the
//! application quietly moved it, and this layer cannot know whether it is.

use std::collections::BTreeSet;

use project_host_database::projects::{self, ProjectRecord};
use project_host_database::Database;
use project_host_project_manager::ports::{PortError, PortPool};

use crate::state::AppState;

/// What is holding a port that a project wanted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Holder {
    /// Another project in this application.
    Project { id: String, display_name: String },
    /// A program on this machine that this application does not manage.
    Program(String),
    /// Something has it and this machine would not say what.
    ///
    /// Not merged into `Program`: an honest "could not find out" reads
    /// differently from a name, and inventing one would be worse than either.
    Unknown,
}

/// A port a project cannot have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    pub port: u16,
    pub holder: Holder,
}

impl Conflict {
    /// The sentence a user reads. Ends by saying what can be done, because a
    /// refusal that only refuses is a dead end.
    pub fn message(&self) -> String {
        let who = match &self.holder {
            Holder::Project { display_name, .. } => {
                format!("the project “{display_name}” is using it")
            }
            Holder::Program(program) => format!("it is held by {program}"),
            Holder::Unknown => "another program on this machine is holding it".to_string(),
        };
        format!(
            "Port {} is not available: {who}. \
             Stop whatever is using it, or give this project a different port.",
            self.port
        )
    }
}

impl std::fmt::Display for Conflict {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

/// How long to keep re-testing a port that reads as busy.
///
/// A restart stops the old process and starts a new one, and the operating
/// system does not always hand the port back on the same tick that the process
/// exits. Without this a restart would refuse itself, blaming the project for
/// holding its own port.
const RELEASE_GRACE: std::time::Duration = std::time::Duration::from_millis(1500);

/// Check every port this project wants, and say who has one if any is taken.
///
/// `Ok(())` means every port bound cleanly a moment ago. That is not a
/// reservation — nothing can hold a port open on the project's behalf without
/// being the project — so a race with another program starting in the same
/// second is still possible, and is caught by the process failing to bind with
/// its own error attached. What this removes is the overwhelmingly common case:
/// a port that was already busy before Start was pressed.
pub async fn check(
    db: &Database,
    project: &ProjectRecord,
) -> Result<(), Box<Conflict>> {
    let ports = match projects::list_ports(db, &project.id).await {
        Ok(ports) => ports,
        // A project whose ports cannot be read is not a port conflict. The
        // start will fail for its own reasons and report them.
        Err(_) => return Ok(()),
    };

    for record in ports {
        let Some(port) = record.host_port.and_then(|value| u16::try_from(value).ok()) else {
            continue;
        };
        if wait_for_release(port).await {
            continue;
        }
        return Err(Box::new(Conflict {
            port,
            holder: identify(db, port, &project.id).await,
        }));
    }

    Ok(())
}

/// Poll a busy port for a moment, in case it is on its way to being free.
///
/// `true` means the port became bindable.
async fn wait_for_release(port: u16) -> bool {
    if project_host_platform::is_free(port) {
        return true;
    }

    let deadline = std::time::Instant::now() + RELEASE_GRACE;
    while std::time::Instant::now() < deadline {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        if project_host_platform::is_free(port) {
            return true;
        }
    }
    false
}

/// Work out who holds `port`.
///
/// Another project is asked about first, and deliberately: it is both the more
/// likely answer once several projects are running and the more useful one,
/// because it names something the user can stop from inside this application.
/// The operating system is only consulted when the database has nothing to say.
async fn identify(db: &Database, port: u16, starting: &str) -> Holder {
    if let Ok(Some(other)) = projects::project_holding_port(db, port, starting).await {
        return Holder::Project {
            id: other.id,
            display_name: other.display_name,
        };
    }

    // A subprocess, so only ever on this path. `None` means the lookup could
    // not answer, which is reported as such rather than guessed at.
    match project_host_platform::owner_of(port) {
        Some(owner) => Holder::Program(owner.describe()),
        None => Holder::Unknown,
    }
}

/// A port this project could have instead.
///
/// Both halves of "free" are checked, for the reason `project-manager`'s
/// allocator gives: the database knows what this application has handed out,
/// and only a bind test knows what the rest of the machine is holding.
pub async fn suggest_free(app: &AppState, project_id: &str) -> Result<u16, PortError> {
    let db = app.database();
    let taken: BTreeSet<u16> = projects::allocated_host_ports(db)
        .await
        .unwrap_or_default()
        .into_iter()
        // The project's own current port is not a reason to skip it, but it is
        // also the port that just failed, so leaving it in `taken` is right:
        // suggesting the port that is already refused would be no suggestion.
        .collect();

    let _ = project_id;
    PortPool::new(app.config().port_pool_start, app.config().port_pool_end).allocate(&taken)
}

/// Give a project a different port, at the user's request.
///
/// Validated against the configured pool rather than merely against 1–65535: a
/// port outside it would collide with something the allocator will later hand
/// out, and the refusal names the range.
pub async fn assign(app: &AppState, project_id: &str, port: u16) -> Result<(), String> {
    let pool = PortPool::new(app.config().port_pool_start, app.config().port_pool_end);
    pool.validate_requested(port).map_err(|error| error.to_string())?;

    if !project_host_platform::is_free(port) {
        let holder = identify(app.database(), port, project_id).await;
        return Err(Conflict { port, holder }.message());
    }

    projects::set_primary_host_port(app.database(), project_id, port)
        .await
        .map_err(|error| {
            // The `UNIQUE (host_port, protocol, bind_address)` constraint is
            // what this hits when another project already claims the port, and
            // the constraint's own words are not a sentence for a person.
            format!(
                "Port {port} could not be assigned: another project already claims it. ({error})"
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_conflict_with_another_project_names_that_project() {
        let conflict = Conflict {
            port: 20001,
            holder: Holder::Project {
                id: "prj_abc".to_string(),
                display_name: "Website".to_string(),
            },
        };

        let message = conflict.message();
        assert!(message.contains("20001"), "{message}");
        assert!(message.contains("Website"), "{message}");
        // A refusal that only refuses is a dead end.
        assert!(message.contains("different port"), "{message}");
    }

    #[test]
    fn a_conflict_with_another_program_names_the_program() {
        let message = Conflict {
            port: 3000,
            holder: Holder::Program("node.exe (pid 24180)".to_string()),
        }
        .message();

        assert!(message.contains("node.exe"), "{message}");
        assert!(message.contains("pid 24180"), "{message}");
    }

    /// "Could not find out" must read as that, not as an invented name.
    #[test]
    fn an_unidentified_holder_says_so_rather_than_guessing() {
        let message = Conflict {
            port: 8080,
            holder: Holder::Unknown,
        }
        .message();

        assert!(message.contains("another program on this machine"), "{message}");
        assert!(!message.contains("pid"), "{message}");
    }

    /// A free port passes the check instantly rather than paying the grace.
    #[tokio::test]
    async fn a_free_port_is_not_waited_on() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        drop(listener);

        let started = std::time::Instant::now();
        assert!(wait_for_release(port).await);
        assert!(
            started.elapsed() < std::time::Duration::from_millis(500),
            "a free port cost the whole grace period"
        );
    }

    /// A port that is genuinely held gives up after the grace rather than
    /// waiting forever, and the grace is short enough not to be felt.
    #[tokio::test]
    async fn a_held_port_is_given_up_on_after_the_grace() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        let started = std::time::Instant::now();
        assert!(!wait_for_release(port).await);
        assert!(
            started.elapsed() >= RELEASE_GRACE,
            "it gave up before the grace was over"
        );
        assert!(
            started.elapsed() < RELEASE_GRACE * 3,
            "it waited far longer than the grace"
        );

        drop(listener);
    }

    /// A port released mid-wait is picked up. This is the restart case: the old
    /// process has not quite let go when the new one asks.
    #[tokio::test]
    async fn a_port_released_during_the_grace_is_accepted() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();

        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
            drop(listener);
        });

        assert!(
            wait_for_release(port).await,
            "a restart refused itself because the port had not been handed back yet"
        );
    }
}
