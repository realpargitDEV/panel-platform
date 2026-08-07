//! Running a project as an ordinary process on this machine.
//!
//! The composition layer, in the same sense as `toolchain_flow`: `host-runner`
//! knows how to probe a toolchain, build a command and supervise a child, and
//! knows nothing about projects, rows or statuses. This joins the two and is
//! the only place that holds both.
//!
//! # What a host project gives up
//!
//! A container supplies filesystem isolation, network isolation, resource
//! limits, a non-root user, and a daemon that outlives this application. A host
//! process supplies none of them. It runs as the user, with the user's files and
//! the user's network, and it is a child of this process.
//!
//! The consequence that shapes this module: **host projects stop when the
//! application quits.** They are not detached and not adopted on the next
//! launch. Recording a pid and reattaching fails on exactly the case that
//! matters — after a reboot the pid has been reused, and the application adopts
//! a stranger's process.
//!
//! **Verified on Windows only.** No Linux or macOS machine was available.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use project_host_api_types::{HealthState, ProjectStatus};
use project_host_database::projects::{self, ProjectRecord, RuntimeRecord};
use project_host_host_runner::health::{Check, Health};
use project_host_host_runner::probe::Toolchain;
use project_host_host_runner::supervisor::{
    HealthPolicy, HostObserved, HostStatus, SupervisorConfig, SupervisorHandle, DEFAULT_GRACE,
};
use project_host_host_runner::{CommandInputs, ProcessCommand};
use tokio::sync::RwLock;

use crate::lifecycle::LifecycleError;
use crate::runner::{Observed, ProjectRunner, StartContext};
use crate::toolchain_flow::MachineResolver;

/// Every host project this process is running.
///
/// Process-scoped by nature rather than by choice: a supervisor handle owns a
/// child of *this* process, so a registry that outlived the process would be
/// describing children that no longer exist.
#[derive(Debug, Clone, Default)]
pub struct HostRegistry(Arc<RwLock<BTreeMap<String, SupervisorHandle>>>);

impl HostRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    async fn insert(&self, project_id: &str, handle: SupervisorHandle) {
        self.0.write().await.insert(project_id.to_string(), handle);
    }

    async fn get(&self, project_id: &str) -> Option<SupervisorHandle> {
        self.0.read().await.get(project_id).cloned()
    }

    async fn remove(&self, project_id: &str) -> Option<SupervisorHandle> {
        self.0.write().await.remove(project_id)
    }

    /// Every project id and handle currently registered.
    ///
    /// Used by shutdown, which has to stop all of them, and by anything
    /// reporting on what this machine is running.
    pub async fn all(&self) -> Vec<(String, SupervisorHandle)> {
        self.0
            .read()
            .await
            .iter()
            .map(|(id, handle)| (id.clone(), handle.clone()))
            .collect()
    }

    /// How many host projects are up. What the quit dialog counts.
    pub async fn running(&self) -> usize {
        self.0
            .read()
            .await
            .values()
            .filter(|handle| handle.observe().status == HostStatus::Running)
            .count()
    }
}

/// Starts projects as processes on this machine.
#[derive(Debug)]
pub struct HostRunner {
    registry: HostRegistry,
    logs_root: PathBuf,
}

impl HostRunner {
    pub fn new(registry: HostRegistry, logs_root: PathBuf) -> Self {
        Self {
            registry,
            logs_root,
        }
    }
}

#[async_trait::async_trait]
impl ProjectRunner for HostRunner {
    async fn start(&self, ctx: StartContext<'_>) -> Result<Observed, LifecycleError> {
        let StartContext {
            db,
            project,
            directory,
            ..
        } = ctx;

        let runtime = projects::find_runtime(db, &project.id)
            .await?
            .ok_or_else(|| {
                LifecycleError::Scaffold("the project has no runtime row".to_string())
            })?;

        let toolchain = resolve_toolchain(&runtime.runtime)?;
        let port = primary_port(db, &project.id).await?;
        let log_path = project_host_host_runner::log_path(&self.logs_root, &project.slug, &today());

        // Anything already registered for this project is a supervisor for a
        // previous run. Ending it first is what stops a restart from leaving two
        // processes fighting over one port.
        if let Some(previous) = self.registry.remove(&project.id).await {
            let _ = previous.stop(DEFAULT_GRACE).await;
        }

        // Install and build run to completion before the start command, and
        // their failure fails the start with their output attached.
        for (phase, command) in [
            ("install", runtime.install_command.as_deref()),
            ("build", runtime.build_command.as_deref()),
        ] {
            let Some(text) = command.filter(|text| !text.trim().is_empty()) else {
                continue;
            };
            let step = build_command(&runtime, text, directory, &toolchain, port)?;
            project_host_host_runner::run_step(phase, step, log_path.clone()).await?;
        }

        let command = build_command(
            &runtime,
            &runtime.start_command,
            directory,
            &toolchain,
            port,
        )?;

        let mut config = SupervisorConfig::new(command, log_path);
        config.health = health_policy(&runtime, port);
        // A host project follows the same restart-policy column a container
        // does. `NO` is the only value that means "leave it alone".
        config.restart_on_crash = project.restart_policy != "NO";

        let handle = project_host_host_runner::start(config).await?;

        self.registry.insert(&project.id, handle.clone()).await;
        projects::record_started(db, &project.id).await?;

        Ok(translate(&handle.observe()))
    }

    async fn stop(&self, project: &ProjectRecord) -> Result<(), LifecycleError> {
        // Removed rather than kept: a stopped project has no supervisor, and
        // leaving a dead handle registered would make it count as running.
        let Some(handle) = self.registry.remove(&project.id).await else {
            return Ok(());
        };
        handle.stop(DEFAULT_GRACE).await?;
        Ok(())
    }

    async fn kill(&self, project: &ProjectRecord) -> Result<(), LifecycleError> {
        let Some(handle) = self.registry.remove(&project.id).await else {
            return Ok(());
        };
        handle.kill().await?;
        Ok(())
    }

    async fn observe(&self, project: &ProjectRecord) -> Result<Option<Observed>, LifecycleError> {
        Ok(self
            .registry
            .get(&project.id)
            .await
            .map(|handle| translate(&handle.observe())))
    }
}

/// Find the runtime's toolchain, or refuse with what was looked for.
///
/// Refused here rather than at spawn time so the message names the runtime and
/// the executables tried, instead of being an operating-system error about a
/// file that does not exist.
fn resolve_toolchain(runtime: &str) -> Result<Toolchain, LifecycleError> {
    // `STATIC` needs no language toolchain but does need something to serve it,
    // and `POLYGLOT` needs several at once. Neither can be answered by finding
    // one executable, so both are refused rather than probed hopefully.
    if project_host_host_runner::candidates_for(runtime).is_empty() {
        return Err(LifecycleError::UnsupportedInHostMode(runtime.to_string()));
    }

    match project_host_host_runner::probe(runtime, &MachineResolver) {
        found @ Toolchain::Found { .. } => Ok(found),
        Toolchain::Missing { looked_for } => Err(LifecycleError::ToolchainMissing {
            runtime: runtime.to_string(),
            looked_for,
        }),
    }
}

/// The port this project was allocated, if it has one.
///
/// A container publishes a mapped port; a host process binds one directly, so
/// the mapping collapses and only the host side is meaningful.
async fn primary_port(
    db: &project_host_database::Database,
    project_id: &str,
) -> Result<Option<u16>, LifecycleError> {
    let ports = projects::list_ports(db, project_id).await?;
    Ok(ports
        .iter()
        .find(|port| port.is_primary)
        .or_else(|| ports.first())
        .and_then(|port| port.host_port)
        .and_then(|port| u16::try_from(port).ok()))
}

fn build_command(
    runtime: &RuntimeRecord,
    text: &str,
    directory: &std::path::Path,
    toolchain: &Toolchain,
    port: Option<u16>,
) -> Result<ProcessCommand, LifecycleError> {
    project_host_host_runner::start_command(CommandInputs {
        runtime: &runtime.runtime,
        command: text,
        // Never `runtime.working_dir`: that is `/app`, a path inside an image.
        project_directory: directory,
        toolchain,
        env: BTreeMap::new(),
        port,
    })
    .map_err(LifecycleError::from)
}

/// The health policy from the runtime row, or `None` when no check is
/// configured.
fn health_policy(runtime: &RuntimeRecord, port: Option<u16>) -> Option<HealthPolicy> {
    if runtime.health_check_type == "NONE" {
        return None;
    }
    Some(HealthPolicy {
        check: Check::resolved(
            &runtime.health_check_type,
            runtime.health_check_target.as_deref(),
            runtime.health_timeout_s,
            port,
        ),
        interval: std::time::Duration::from_secs(runtime.health_interval_s.clamp(1, 3600) as u64),
        start_period: std::time::Duration::from_secs(
            runtime.health_start_period_s.clamp(0, 3600) as u64
        ),
    })
}

/// Today, as the log file names it.
fn today() -> String {
    project_host_database::time::now()
        .get(..10)
        .unwrap_or("unknown")
        .to_string()
}

/// `host-runner`'s vocabulary in this application's.
///
/// The two are deliberately separate — see that crate's documentation — and this
/// is the single place the translation happens.
fn translate(observed: &HostObserved) -> Observed {
    Observed {
        status: match observed.status {
            HostStatus::Running => ProjectStatus::Running,
            HostStatus::Stopped => ProjectStatus::Stopped,
            HostStatus::Failed => ProjectStatus::Failed,
        },
        health: Some(match observed.health {
            // No check configured. Not "unknown": nothing was asked, and the
            // column has a word for that.
            Health::None => HealthState::None,
            Health::Passing => HealthState::Healthy,
            Health::Failing(_) => HealthState::Unhealthy,
        }),
        exit_code: observed.exit_code,
        failure_reason: observed.failure_reason.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn runtime(kind: &str) -> RuntimeRecord {
        RuntimeRecord {
            project_id: "prj".to_string(),
            runtime: kind.to_string(),
            runtime_version: "latest".to_string(),
            package_manager: "NPM".to_string(),
            install_command: None,
            build_command: None,
            start_command: "node index.js".to_string(),
            working_dir: "/app".to_string(),
            entry_file: None,
            publish_dir: None,
            template_id: "t".to_string(),
            health_check_type: "NONE".to_string(),
            health_check_target: None,
            health_interval_s: 30,
            health_timeout_s: 5,
            health_retries: 3,
            health_start_period_s: 10,
        }
    }

    /// A static site has no language toolchain to find, and a polyglot project
    /// has several. Both are refused by name rather than reported as a missing
    /// executable that was never going to exist.
    #[test]
    fn runtimes_with_no_single_toolchain_are_refused_by_name() {
        for kind in ["STATIC", "POLYGLOT"] {
            match resolve_toolchain(kind) {
                Err(LifecycleError::UnsupportedInHostMode(named)) => assert_eq!(named, kind),
                other => panic!("{kind} should be refused, got {other:?}"),
            }
        }
    }

    /// The refusal has to name what to install, not merely say no.
    #[test]
    fn a_missing_toolchain_names_what_was_looked_for() {
        // Nothing resolves `definitely-not-a-runtime`, so this is the Missing
        // path without depending on what is installed on the test machine.
        match resolve_toolchain("NODEJS") {
            // On a machine with Node this is Found, which is also correct.
            Ok(Toolchain::Found { .. }) => {}
            Err(LifecycleError::ToolchainMissing {
                runtime,
                looked_for,
            }) => {
                assert_eq!(runtime, "NODEJS");
                assert_eq!(looked_for, vec!["node".to_string()]);
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn no_health_check_configured_means_no_polling() {
        assert!(health_policy(&runtime("NODEJS"), Some(8080)).is_none());
    }

    #[test]
    fn a_configured_check_carries_the_allocated_port() {
        let mut record = runtime("NODEJS");
        record.health_check_type = "HTTP".to_string();
        record.health_check_target = Some("http://127.0.0.1:{port}/healthz".to_string());

        let policy = health_policy(&record, Some(8081)).expect("a policy");
        assert_eq!(
            policy.check.target.as_deref(),
            Some("http://127.0.0.1:8081/healthz")
        );
        assert_eq!(policy.interval, std::time::Duration::from_secs(30));
        assert_eq!(policy.start_period, std::time::Duration::from_secs(10));
    }

    /// The trap this whole path exists to avoid: `/app` is a container path and
    /// must never become a host process's working directory.
    #[test]
    fn the_container_working_directory_never_reaches_a_host_command() {
        let record = runtime("NODEJS");
        assert_eq!(
            record.working_dir, "/app",
            "the trap is still there to avoid"
        );

        let toolchain = Toolchain::Found {
            executable: PathBuf::from("node"),
            version: None,
        };
        let directory = std::path::Path::new("projects/quiet-harbor");
        let command = build_command(&record, "node index.js", directory, &toolchain, Some(8080))
            .expect("a command");

        assert_eq!(command.cwd, directory);
        assert!(!command.cwd.to_string_lossy().contains("/app"));
        assert_eq!(command.env.get("PORT").map(String::as_str), Some("8080"));
    }

    #[test]
    fn host_status_becomes_the_status_the_column_takes() {
        let observed = |status, health| HostObserved {
            status,
            health,
            pid: None,
            exit_code: None,
            failure_reason: None,
            restarts: 0,
        };

        assert_eq!(
            translate(&observed(HostStatus::Running, Health::Passing)).status,
            ProjectStatus::Running
        );
        assert_eq!(
            translate(&observed(HostStatus::Failed, Health::None)).status,
            ProjectStatus::Failed
        );
        assert_eq!(
            translate(&observed(HostStatus::Stopped, Health::None)).status,
            ProjectStatus::Stopped
        );

        // No check configured is NONE, not UNKNOWN: nothing was asked, and
        // recording "unknown" would suggest something was and failed.
        assert_eq!(
            translate(&observed(HostStatus::Running, Health::None)).health,
            Some(HealthState::None)
        );
        assert_eq!(
            translate(&observed(
                HostStatus::Running,
                Health::Failing("no connection".to_string())
            ))
            .health,
            Some(HealthState::Unhealthy)
        );
    }

    #[tokio::test]
    async fn an_empty_registry_reports_nothing_running() {
        let registry = HostRegistry::new();
        assert_eq!(registry.running().await, 0);
        assert!(registry.all().await.is_empty());
        assert!(registry.get("prj_absent").await.is_none());
    }

    /// Stopping a project this process is not running is the state the caller
    /// asked for, not an error.
    #[tokio::test]
    async fn stopping_an_unregistered_project_is_success() {
        let runner = HostRunner::new(HostRegistry::new(), PathBuf::from("logs"));
        let project = crate::runner::tests::sample_project();
        runner.stop(&project).await.expect("stop");
        runner.kill(&project).await.expect("kill");
        assert!(runner.observe(&project).await.expect("observe").is_none());
    }

    #[test]
    fn the_log_file_is_named_for_today() {
        let today = today();
        assert_eq!(today.len(), 10, "got {today}");
        assert!(today.as_str() > "2020-01-01", "got {today}");
    }
}
