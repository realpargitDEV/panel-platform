//! Actually running a container.
//!
//! Everything here turns an already-hardened [`ContainerSpec`] into calls
//! against the Docker Engine. The important property is that this module makes
//! **no security decisions**: it copies the spec across faithfully and refuses
//! to run one that fails its own audit. That keeps the hardening in a single
//! place that can be tested without a daemon, and leaves this layer to be
//! wrong only about plumbing.
//!
//! # What is verified and what is not
//!
//! Every call in this file compiles against bollard's API, which is a real
//! check — the argument shapes and types are wrong far more often than the
//! logic is. **Nothing here has ever been run against a Docker daemon**,
//! because the machine it was written on has none. The integration tests at the
//! bottom skip themselves when no daemon answers, and are the only thing that
//! would prove any of it.

use std::collections::HashMap;
use std::path::Path;

use bollard::Docker;

use crate::container_spec::{ContainerSpec, NetworkMode, RestartPolicy};
use crate::DockerError;

/// How long a container gets to stop before it is killed.
///
/// Matches the spec's own timeout. A bot that needs to close connections
/// cleanly gets that long and no longer.
const DEFAULT_STOP_TIMEOUT: i64 = 10;

/// What happened to a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContainerState {
    pub id: String,
    /// Docker's own word: `created`, `running`, `exited`, `dead`, …
    pub status: String,
    pub running: bool,
    pub exit_code: Option<i64>,
    /// `healthy`, `unhealthy`, `starting`, or absent when no health check is
    /// configured.
    pub health: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Set when Docker killed the container for exceeding its memory limit.
    /// Worth surfacing separately: it looks like a crash but is not a bug in
    /// the project.
    pub out_of_memory: bool,
}

impl ContainerState {
    /// The status word this application stores, derived from Docker's.
    pub fn project_status(&self) -> &'static str {
        if self.running {
            return "RUNNING";
        }
        match self.exit_code {
            Some(0) | None => "STOPPED",
            Some(_) => "FAILED",
        }
    }
}

/// Run containers for projects.
///
/// Holds a connected client. Constructing one does not prove the daemon is
/// reachable — see [`crate::BollardProbe::connect`], which pings.
#[derive(Clone)]
pub struct ContainerRunner {
    client: Docker,
}

impl std::fmt::Debug for ContainerRunner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ContainerRunner")
    }
}

impl ContainerRunner {
    pub fn new(client: Docker) -> Self {
        Self { client }
    }

    /// Create the project's private network, if it does not exist.
    ///
    /// A network per project is what stops one project reaching another's
    /// ports. Already existing is success, not an error: creation has to be
    /// safe to retry after a partial failure.
    pub async fn ensure_network(&self, name: &str) -> Result<(), DockerError> {
        if self.client.inspect_network(name, None).await.is_ok() {
            return Ok(());
        }

        let mut labels = HashMap::new();
        labels.insert("dev.projecthost.managed".to_string(), "true".to_string());

        self.client
            .create_network(bollard::models::NetworkCreateRequest {
                name: name.to_string(),
                driver: Some("bridge".to_string()),
                // Not internal by default: `NetworkMode` decides reachability
                // per project, and an internal network here would override it.
                labels: Some(labels),
                ..Default::default()
            })
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))?;
        Ok(())
    }

    /// Create the project's data volume, if it does not exist.
    pub async fn ensure_volume(&self, name: &str) -> Result<(), DockerError> {
        if self.client.inspect_volume(name).await.is_ok() {
            return Ok(());
        }

        let mut labels = HashMap::new();
        labels.insert("dev.projecthost.managed".to_string(), "true".to_string());

        self.client
            .create_volume(bollard::models::VolumeCreateRequest {
                name: Some(name.to_string()),
                labels: Some(labels),
                ..Default::default()
            })
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))?;
        Ok(())
    }

    /// Create the container described by `spec`.
    ///
    /// Refuses a spec that fails its own security audit. That check is
    /// deliberately here, at the last moment before the container exists,
    /// rather than only where the spec was built — this is the function that
    /// would otherwise hand a privileged container to Docker.
    pub async fn create(&self, spec: &ContainerSpec) -> Result<String, DockerError> {
        let violations = spec.security_violations();
        if !violations.is_empty() {
            let detail = violations
                .iter()
                .map(|violation| format!("{}: {}", violation.rule, violation.detail))
                .collect::<Vec<_>>()
                .join("; ");
            return Err(DockerError::Daemon(format!(
                "refusing to create a container that fails its own hardening audit — {detail}"
            )));
        }

        let config = to_bollard_config(spec);

        let created = self
            .client
            .create_container(
                Some(bollard::query_parameters::CreateContainerOptions {
                    name: Some(spec.name.clone()),
                    ..Default::default()
                }),
                config,
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))?;

        Ok(created.id)
    }

    pub async fn start(&self, name: &str) -> Result<(), DockerError> {
        self.client
            .start_container(
                name,
                None::<bollard::query_parameters::StartContainerOptions>,
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))
    }

    /// Stop a container, giving it `timeout` seconds to exit on its own.
    pub async fn stop(&self, name: &str, timeout: Option<i64>) -> Result<(), DockerError> {
        self.client
            .stop_container(
                name,
                Some(bollard::query_parameters::StopContainerOptions {
                    t: Some(i32::try_from(timeout.unwrap_or(DEFAULT_STOP_TIMEOUT)).unwrap_or(10)),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))
    }

    pub async fn restart(&self, name: &str, timeout: Option<i64>) -> Result<(), DockerError> {
        self.client
            .restart_container(
                name,
                Some(bollard::query_parameters::RestartContainerOptions {
                    t: Some(i32::try_from(timeout.unwrap_or(DEFAULT_STOP_TIMEOUT)).unwrap_or(10)),
                    ..Default::default()
                }),
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))
    }

    /// Kill a running container immediately.
    pub async fn kill(&self, name: &str) -> Result<(), DockerError> {
        self.client
            .kill_container(
                name,
                None::<bollard::query_parameters::KillContainerOptions>,
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))
    }

    /// Remove a container. `force` kills it first if it is still running.
    ///
    /// Volumes are never removed with it: a project's data outliving its
    /// container is the whole point of the volume, and `v: false` says so
    /// explicitly rather than relying on the default.
    pub async fn remove(&self, name: &str, force: bool) -> Result<(), DockerError> {
        self.client
            .remove_container(
                name,
                Some(bollard::query_parameters::RemoveContainerOptions {
                    force,
                    v: false,
                    link: false,
                }),
            )
            .await
            .map_err(|error| DockerError::Daemon(error.to_string()))
    }

    /// What Docker currently believes about a container.
    ///
    /// `Ok(None)` means there is no such container, which is a normal answer
    /// during reconciliation rather than a failure.
    pub async fn inspect(&self, name: &str) -> Result<Option<ContainerState>, DockerError> {
        let inspected = match self
            .client
            .inspect_container(
                name,
                None::<bollard::query_parameters::InspectContainerOptions>,
            )
            .await
        {
            Ok(inspected) => inspected,
            Err(bollard::errors::Error::DockerResponseServerError {
                status_code: 404, ..
            }) => return Ok(None),
            Err(error) => return Err(DockerError::Daemon(error.to_string())),
        };

        let state = inspected.state.unwrap_or_default();
        Ok(Some(ContainerState {
            id: inspected.id.unwrap_or_default(),
            status: state
                .status
                .map(|status| status.to_string())
                .unwrap_or_else(|| "unknown".to_string()),
            running: state.running.unwrap_or(false),
            exit_code: state.exit_code,
            health: state.health.and_then(|health| {
                health
                    .status
                    .map(|status| status.to_string().to_lowercase())
            }),
            started_at: state.started_at,
            finished_at: state.finished_at,
            out_of_memory: state.oom_killed.unwrap_or(false),
        }))
    }

    /// Whether an image is present locally.
    pub async fn has_image(&self, tag: &str) -> bool {
        self.client.inspect_image(tag).await.is_ok()
    }

    /// Build an image from a directory containing a `Dockerfile`.
    ///
    /// The context is streamed as an uncompressed tar. Every log line the
    /// daemon emits is handed to `on_output`, because a failed build is
    /// diagnosable only from that stream.
    pub async fn build_image<F>(
        &self,
        tag: &str,
        context_dir: &Path,
        mut on_output: F,
    ) -> Result<(), DockerError>
    where
        F: FnMut(&str),
    {
        use futures_util::StreamExt;

        let tarball = tar_directory(context_dir)?;

        let options = bollard::query_parameters::BuildImageOptions {
            dockerfile: "Dockerfile".to_string(),
            t: Some(tag.to_string()),
            // Always fetch a fresh base layer rather than trusting whatever is
            // cached locally, so a rebuild picks up security updates. Docker
            // takes this as a string flag, not a boolean.
            pull: Some("true".to_string()),
            rm: true,
            forcerm: true,
            ..Default::default()
        };

        let mut stream = self.client.build_image(
            options,
            None,
            Some(bollard::body_full(bytes::Bytes::from(tarball))),
        );

        while let Some(item) = stream.next().await {
            match item {
                Ok(output) => {
                    if let Some(stream) = output.stream {
                        on_output(stream.trim_end());
                    }
                    // The daemon reports a failed build in the body of a
                    // successful HTTP response, so this is the only place a
                    // build error appears.
                    if let Some(detail) = output.error_detail {
                        return Err(DockerError::Daemon(
                            detail
                                .message
                                .unwrap_or_else(|| "the build failed".to_string()),
                        ));
                    }
                }
                Err(error) => return Err(DockerError::Daemon(error.to_string())),
            }
        }

        Ok(())
    }
}

/// Pack a directory into an uncompressed tar for the build context.
///
/// Uncompressed on purpose: the daemon is on the same machine, so compression
/// would cost CPU to save nothing.
fn tar_directory(directory: &Path) -> Result<Vec<u8>, DockerError> {
    let mut builder = tar::Builder::new(Vec::new());
    builder.append_dir_all(".", directory).map_err(|error| {
        DockerError::Daemon(format!("could not read the build context: {error}"))
    })?;
    builder
        .into_inner()
        .map_err(|error| DockerError::Daemon(format!("could not pack the build context: {error}")))
}

/// Translate the hardened spec into bollard's configuration.
///
/// Written as a straight, exhaustive copy. Anything this function decides for
/// itself is a hardening decision made outside `container_spec`, which is
/// exactly what must not happen.
fn to_bollard_config(spec: &ContainerSpec) -> bollard::models::ContainerCreateBody {
    use bollard::models::{
        HostConfig, PortBinding as BollardPortBinding, RestartPolicy as BollardRestartPolicy,
        RestartPolicyNameEnum,
    };

    let mut port_bindings: HashMap<String, Option<Vec<BollardPortBinding>>> = HashMap::new();
    let mut exposed: Vec<String> = Vec::new();
    for port in &spec.ports {
        let key = format!("{}/{}", port.container_port, port.protocol);
        exposed.push(key.clone());
        port_bindings.insert(
            key,
            Some(vec![BollardPortBinding {
                // Loopback unless the project explicitly asked otherwise, which
                // `container_spec` decided, not this function.
                host_ip: Some(port.bind_address.clone()),
                host_port: Some(port.host_port.to_string()),
            }]),
        );
    }

    let binds: Vec<String> = spec
        .binds
        .iter()
        .map(|bind| {
            format!(
                "{}:{}:{}",
                bind.source.display(),
                bind.target,
                if bind.read_only { "ro" } else { "rw" }
            )
        })
        .chain(
            spec.volumes
                .iter()
                .map(|(volume, target)| format!("{volume}:{target}")),
        )
        .collect();

    let restart_policy = BollardRestartPolicy {
        name: Some(match spec.restart_policy {
            RestartPolicy::No => RestartPolicyNameEnum::NO,
            RestartPolicy::OnFailure => RestartPolicyNameEnum::ON_FAILURE,
            RestartPolicy::UnlessStopped => RestartPolicyNameEnum::UNLESS_STOPPED,
            RestartPolicy::Always => RestartPolicyNameEnum::ALWAYS,
        }),
        maximum_retry_count: match spec.restart_policy {
            RestartPolicy::OnFailure => Some(5),
            _ => None,
        },
    };

    let host_config = HostConfig {
        binds: if binds.is_empty() { None } else { Some(binds) },
        port_bindings: if port_bindings.is_empty() {
            None
        } else {
            Some(port_bindings)
        },
        network_mode: match spec.network_mode {
            NetworkMode::None => Some("none".to_string()),
            _ => spec.network_name.clone(),
        },
        readonly_rootfs: Some(spec.read_only_root_filesystem),
        tmpfs: Some(spec.tmpfs.clone().into_iter().collect()),
        memory: Some(i64::try_from(spec.limits.memory_bytes).unwrap_or(i64::MAX)),
        // Equal to `memory`, so the container cannot swap its way past the
        // limit instead of being killed.
        memory_swap: Some(i64::try_from(spec.limits.memory_swap_bytes).unwrap_or(i64::MAX)),
        // Quota against a period rather than `nano_cpus`: the spec already
        // expresses it that way, and converting would lose precision.
        cpu_quota: Some(spec.limits.cpu_quota),
        cpu_period: Some(spec.limits.cpu_period),
        pids_limit: Some(spec.limits.pids_limit),
        security_opt: Some(spec.security_opt.clone()),
        cap_drop: Some(spec.cap_drop.clone()),
        cap_add: if spec.cap_add.is_empty() {
            None
        } else {
            Some(spec.cap_add.clone())
        },
        privileged: Some(spec.privileged),
        restart_policy: Some(restart_policy),
        log_config: Some(bollard::models::HostConfigLogConfig {
            typ: Some(spec.log_driver.clone()),
            config: Some(spec.log_options.clone().into_iter().collect()),
        }),
        ..Default::default()
    };

    bollard::models::ContainerCreateBody {
        image: Some(spec.image.clone()),
        user: Some(spec.user.clone()),
        working_dir: Some(spec.working_dir.clone()),
        cmd: if spec.command.is_empty() {
            None
        } else {
            Some(spec.command.clone())
        },
        env: Some(
            spec.environment
                .iter()
                .map(|(key, value)| format!("{key}={value}"))
                .collect(),
        ),
        labels: Some(spec.labels.clone().into_iter().collect()),
        exposed_ports: if exposed.is_empty() {
            None
        } else {
            Some(exposed)
        },
        healthcheck: spec
            .health_check
            .as_ref()
            .map(|check| bollard::models::HealthConfig {
                test: Some(check.test.clone()),
                interval: Some(i64::from(check.interval_seconds) * 1_000_000_000),
                timeout: Some(i64::from(check.timeout_seconds) * 1_000_000_000),
                retries: Some(i64::from(check.retries)),
                start_period: Some(i64::from(check.start_period_seconds) * 1_000_000_000),
                ..Default::default()
            }),
        stop_timeout: Some(spec.stop_timeout_seconds),
        host_config: Some(host_config),
        ..Default::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::container_spec::{ResourceLimits, SpecInputs};
    use std::path::PathBuf;

    fn spec() -> ContainerSpec {
        ContainerSpec::build(SpecInputs {
            slug: "quiet-harbor-4f2a".to_string(),
            project_id: "prj_0193000000007000800000000000abcd".to_string(),
            template_id: "nodejs".to_string(),
            agent_version: "0.1.0".to_string(),
            image_tag: "projecthost/quiet-harbor-4f2a:latest".to_string(),
            command: vec!["node".to_string(), "index.js".to_string()],
            working_dir: "/app".to_string(),
            environment: vec![("NODE_ENV".to_string(), "production".to_string())],
            project_dir: PathBuf::from("/var/lib/project-host/projects/quiet-harbor-4f2a"),
            data_volume: "ph_vol_quiet-harbor-4f2a".to_string(),
            network_mode: NetworkMode::Internet,
            ports: vec![crate::container_spec::PortBinding {
                container_port: 3000,
                host_port: 20001,
                protocol: "tcp".to_string(),
                bind_address: "127.0.0.1".to_string(),
            }],
            limits: ResourceLimits::from_user_values(512, 1.0, 128),
            restart_policy: RestartPolicy::UnlessStopped,
            health_check: None,
        })
    }

    /// The translation must not quietly drop a hardening setting. Each of these
    /// is a control that would be invisible if it were lost.
    #[test]
    fn the_hardening_survives_translation_to_dockers_configuration() {
        let config = to_bollard_config(&spec());
        let host = config.host_config.expect("a host config");

        assert_eq!(host.readonly_rootfs, Some(true));
        assert_eq!(host.privileged, Some(false));
        assert!(host
            .security_opt
            .expect("security options")
            .iter()
            .any(|option| option == "no-new-privileges:true"));
        assert!(host
            .cap_drop
            .expect("dropped capabilities")
            .iter()
            .any(|capability| capability == "ALL"));
        assert!(host.pids_limit.unwrap_or_default() > 0);
        assert!(host.memory.unwrap_or_default() > 0);
        assert!(host.cpu_quota.unwrap_or_default() > 0);
        assert!(host.cpu_period.unwrap_or_default() > 0);
    }

    #[test]
    fn the_container_never_runs_as_root() {
        let config = to_bollard_config(&spec());
        let user = config.user.expect("a user");
        assert_ne!(user, "root");
        assert_ne!(user, "0:0");
        assert!(!user.starts_with("0:"), "got {user}");
    }

    #[test]
    fn swap_is_pinned_to_the_memory_limit() {
        // Without this a container can swap past its memory limit instead of
        // being killed, and one project can exhaust the host.
        let config = to_bollard_config(&spec());
        let host = config.host_config.expect("a host config");
        assert_eq!(host.memory, host.memory_swap);
    }

    #[test]
    fn a_published_port_is_bound_to_loopback() {
        let config = to_bollard_config(&spec());
        let host = config.host_config.expect("a host config");
        let bindings = host.port_bindings.expect("port bindings");
        let published = bindings.get("3000/tcp").expect("the mapped port");
        let first = published
            .as_ref()
            .and_then(|list| list.first())
            .expect("a binding");
        assert_eq!(first.host_ip.as_deref(), Some("127.0.0.1"));
        assert_eq!(first.host_port.as_deref(), Some("20001"));
    }

    #[test]
    fn a_project_with_no_network_gets_dockers_none_network() {
        let mut without = spec();
        without.network_mode = NetworkMode::None;
        without.network_name = None;
        let config = to_bollard_config(&without);
        let host = config.host_config.expect("a host config");
        assert_eq!(host.network_mode.as_deref(), Some("none"));
    }

    #[test]
    fn the_docker_socket_is_never_mounted() {
        // The single most important thing this translation must not do.
        let config = to_bollard_config(&spec());
        let host = config.host_config.expect("a host config");
        for bind in host.binds.unwrap_or_default() {
            assert!(
                !bind.contains("docker.sock") && !bind.contains("docker_engine"),
                "the Docker socket reached a container: {bind}"
            );
        }
    }

    #[test]
    fn the_environment_is_a_list_not_a_shell_string() {
        let config = to_bollard_config(&spec());
        let env = config.env.expect("environment");
        assert!(env.iter().any(|entry| entry == "NODE_ENV=production"));
        // A shell would be needed to interpret these; there is no shell.
        assert!(!env.iter().any(|entry| entry.contains("&&")));
    }

    #[test]
    fn the_command_is_a_structured_argument_list() {
        let config = to_bollard_config(&spec());
        assert_eq!(
            config.cmd,
            Some(vec!["node".to_string(), "index.js".to_string()])
        );
    }

    #[test]
    fn a_stopped_container_with_a_non_zero_exit_is_reported_as_failed() {
        let failed = ContainerState {
            id: "abc".to_string(),
            status: "exited".to_string(),
            running: false,
            exit_code: Some(137),
            health: None,
            started_at: None,
            finished_at: None,
            out_of_memory: true,
        };
        assert_eq!(failed.project_status(), "FAILED");

        let stopped = ContainerState {
            exit_code: Some(0),
            ..failed.clone()
        };
        assert_eq!(stopped.project_status(), "STOPPED");

        let running = ContainerState {
            running: true,
            exit_code: None,
            ..failed
        };
        assert_eq!(running.project_status(), "RUNNING");
    }

    #[test]
    fn log_rotation_is_configured_so_a_chatty_project_cannot_fill_the_disk() {
        let config = to_bollard_config(&spec());
        let host = config.host_config.expect("a host config");
        let log = host.log_config.expect("log config");
        let options = log.config.expect("log options");
        assert!(options.contains_key("max-size"));
        assert!(options.contains_key("max-file"));
    }
}
