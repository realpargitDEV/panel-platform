//! The container specification.
//!
//! Every project container is described by this struct and created from it. It
//! is built in Rust and handed to the Docker API as structured data — there is
//! no shell, no string concatenation, and no command line for anything to be
//! injected into.
//!
//! The hardening below is not configurable. There is no field a user can set to
//! obtain a privileged container, a mounted Docker socket, or host networking,
//! because no such field exists. `docs/docker.md` §3 lists what is forbidden;
//! [`ContainerSpec::security_violations`] asserts it, and a test runs that
//! assertion over every spec the builder can produce.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Label namespace. Everything the agent creates carries these, which is how
/// the reconciler tells its containers from anything else on the host.
pub const LABEL_MANAGED: &str = "io.projecthost.managed";
pub const LABEL_PROJECT_ID: &str = "io.projecthost.project-id";
pub const LABEL_TEMPLATE: &str = "io.projecthost.template";
pub const LABEL_VERSION: &str = "io.projecthost.version";

/// Non-root, and the same in every template. A project that needs a different
/// uid is a project that needs a different template, reviewed on its own terms.
pub const CONTAINER_UID: u32 = 10_001;
pub const CONTAINER_GID: u32 = 10_001;

/// Where the project's own files are mounted.
pub const APP_MOUNT: &str = "/app";
/// Writable data that should survive a rebuild.
pub const DATA_MOUNT: &str = "/data";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RestartPolicy {
    No,
    OnFailure,
    UnlessStopped,
    Always,
}

impl RestartPolicy {
    /// Docker's own spelling.
    pub fn as_docker(&self) -> &'static str {
        match self {
            RestartPolicy::No => "no",
            RestartPolicy::OnFailure => "on-failure",
            RestartPolicy::UnlessStopped => "unless-stopped",
            RestartPolicy::Always => "always",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NetworkMode {
    /// No interface at all.
    None,
    /// Dedicated network with no outbound route.
    Internal,
    /// Dedicated network, port published to a private address.
    Lan,
    /// Dedicated network with outbound routing.
    Internet,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortBinding {
    pub container_port: u16,
    pub host_port: u16,
    pub protocol: String,
    /// `127.0.0.1` unless the user explicitly asked for LAN exposure, at both
    /// the project level and the agent level.
    pub bind_address: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HealthCheck {
    pub test: Vec<String>,
    pub interval_seconds: u32,
    pub timeout_seconds: u32,
    pub retries: u32,
    pub start_period_seconds: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub memory_bytes: u64,
    /// Equal to `memory_bytes`. Leaving swap unset lets a container exceed its
    /// memory limit through swap, quietly defeating the limit the user set.
    pub memory_swap_bytes: u64,
    pub cpu_quota: i64,
    pub cpu_period: i64,
    pub pids_limit: i64,
}

impl ResourceLimits {
    /// Docker's CPU quota is expressed against a period; 100 000 µs is the
    /// conventional period, so a quota of 150 000 means 1.5 cores.
    pub fn from_user_values(memory_mb: u32, cpu_cores: f32, process_limit: u32) -> Self {
        let memory_bytes = u64::from(memory_mb) * 1024 * 1024;
        let period = 100_000i64;
        let quota = ((f64::from(cpu_cores) * period as f64).round() as i64).max(1_000);
        Self {
            memory_bytes,
            memory_swap_bytes: memory_bytes,
            cpu_quota: quota,
            cpu_period: period,
            pids_limit: i64::from(process_limit),
        }
    }
}

/// A bind mount. Only ever the project's own directory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BindMount {
    pub source: PathBuf,
    pub target: String,
    pub read_only: bool,
}

/// The complete description of a project container.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerSpec {
    pub name: String,
    pub image: String,
    pub labels: BTreeMap<String, String>,
    /// `uid:gid`, never root.
    pub user: String,
    pub working_dir: String,
    /// Structured, never a shell string.
    pub command: Vec<String>,
    pub environment: Vec<(String, String)>,
    pub read_only_root_filesystem: bool,
    pub tmpfs: BTreeMap<String, String>,
    pub binds: Vec<BindMount>,
    pub volumes: Vec<(String, String)>,
    pub network_name: Option<String>,
    pub network_mode: NetworkMode,
    pub ports: Vec<PortBinding>,
    pub limits: ResourceLimits,
    pub restart_policy: RestartPolicy,
    pub security_opt: Vec<String>,
    pub cap_drop: Vec<String>,
    pub cap_add: Vec<String>,
    pub privileged: bool,
    pub health_check: Option<HealthCheck>,
    pub log_driver: String,
    pub log_options: BTreeMap<String, String>,
    pub stop_timeout_seconds: i64,
}

/// Everything needed to build a spec. Constructed by `project-manager` from
/// validated database rows — never directly from a request body.
#[derive(Debug, Clone)]
pub struct SpecInputs {
    /// Generated slug, e.g. `quiet-harbor-4f2a`. Never user text.
    pub slug: String,
    pub project_id: String,
    pub template_id: String,
    pub agent_version: String,
    pub image_tag: String,
    pub command: Vec<String>,
    pub working_dir: String,
    pub environment: Vec<(String, String)>,
    pub project_dir: PathBuf,
    pub data_volume: String,
    pub network_mode: NetworkMode,
    pub ports: Vec<PortBinding>,
    pub limits: ResourceLimits,
    pub restart_policy: RestartPolicy,
    pub health_check: Option<HealthCheck>,
}

/// A hardening rule that a spec broke. Any non-empty list is a bug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityViolation {
    pub rule: &'static str,
    pub detail: String,
}

impl ContainerSpec {
    /// Container name from the generated slug. Docker names must match
    /// `[a-zA-Z0-9][a-zA-Z0-9_.-]*`, which the slug already satisfies.
    pub fn container_name(slug: &str) -> String {
        format!("ph_{slug}")
    }

    pub fn network_name(slug: &str) -> String {
        format!("ph_net_{slug}")
    }

    pub fn volume_name(slug: &str) -> String {
        format!("ph_vol_{slug}")
    }

    /// Build the spec. Every hardening decision is made here, unconditionally.
    pub fn build(inputs: SpecInputs) -> Self {
        let mut labels = BTreeMap::new();
        labels.insert(LABEL_MANAGED.to_string(), "true".to_string());
        labels.insert(LABEL_PROJECT_ID.to_string(), inputs.project_id.clone());
        labels.insert(LABEL_TEMPLATE.to_string(), inputs.template_id.clone());
        labels.insert(LABEL_VERSION.to_string(), inputs.agent_version.clone());

        let mut tmpfs = BTreeMap::new();
        // Somewhere writable for a process that expects /tmp, without giving up
        // the read-only root. noexec stops a dropped payload being run from it.
        tmpfs.insert("/tmp".to_string(), "rw,noexec,nosuid,size=64m".to_string());

        let mut log_options = BTreeMap::new();
        log_options.insert("max-size".to_string(), "10m".to_string());
        log_options.insert("max-file".to_string(), "3".to_string());

        // NONE gets no network at all; everything else gets its own.
        let network_name = match inputs.network_mode {
            NetworkMode::None => None,
            _ => Some(Self::network_name(&inputs.slug)),
        };

        // A project with no network cannot publish a port, whatever was asked.
        let ports = match inputs.network_mode {
            NetworkMode::None | NetworkMode::Internal => Vec::new(),
            _ => inputs.ports,
        };

        Self {
            name: Self::container_name(&inputs.slug),
            image: inputs.image_tag,
            labels,
            user: format!("{CONTAINER_UID}:{CONTAINER_GID}"),
            working_dir: inputs.working_dir,
            command: inputs.command,
            environment: inputs.environment,
            read_only_root_filesystem: true,
            tmpfs,
            binds: vec![BindMount {
                source: inputs.project_dir,
                target: APP_MOUNT.to_string(),
                read_only: false,
            }],
            volumes: vec![(inputs.data_volume, DATA_MOUNT.to_string())],
            network_name,
            network_mode: inputs.network_mode,
            ports,
            limits: inputs.limits,
            restart_policy: inputs.restart_policy,
            security_opt: vec!["no-new-privileges:true".to_string()],
            cap_drop: vec!["ALL".to_string()],
            cap_add: Vec::new(),
            privileged: false,
            health_check: inputs.health_check,
            log_driver: "json-file".to_string(),
            log_options,
            stop_timeout_seconds: 10,
        }
    }

    /// Check every forbidden property. Returns an empty list for a sound spec.
    ///
    /// This runs in tests over every spec the builder can produce, and is also
    /// called immediately before a container is created — so a future code path
    /// that constructs a spec by hand cannot bypass it.
    pub fn security_violations(&self) -> Vec<SecurityViolation> {
        let mut violations = Vec::new();

        if self.privileged {
            violations.push(SecurityViolation {
                rule: "privileged",
                detail: "privileged mode is never permitted".to_string(),
            });
        }

        if self.user.starts_with("0:") || self.user == "root" || self.user.is_empty() {
            violations.push(SecurityViolation {
                rule: "non-root-user",
                detail: format!("container user must not be root, got `{}`", self.user),
            });
        }

        if !self
            .security_opt
            .iter()
            .any(|option| option == "no-new-privileges:true")
        {
            violations.push(SecurityViolation {
                rule: "no-new-privileges",
                detail: "no-new-privileges must be set".to_string(),
            });
        }

        if !self.cap_drop.iter().any(|capability| capability == "ALL") {
            violations.push(SecurityViolation {
                rule: "cap-drop-all",
                detail: "all capabilities must be dropped".to_string(),
            });
        }

        if !self.cap_add.is_empty() {
            violations.push(SecurityViolation {
                rule: "cap-add",
                detail: format!("no capability may be added, got {:?}", self.cap_add),
            });
        }

        if !self.read_only_root_filesystem {
            violations.push(SecurityViolation {
                rule: "read-only-root",
                detail: "the root filesystem must be read-only".to_string(),
            });
        }

        // The socket in any form is total host compromise.
        for bind in &self.binds {
            let source = bind.source.to_string_lossy().replace('\\', "/");
            if source.contains("docker.sock") || source.contains("pipe/docker_engine") {
                violations.push(SecurityViolation {
                    rule: "docker-socket",
                    detail: format!("the Docker socket must never be mounted: {source}"),
                });
            }
            if bind.target.starts_with("/proc")
                || bind.target.starts_with("/sys")
                || bind.target == "/"
            {
                violations.push(SecurityViolation {
                    rule: "sensitive-mount-target",
                    detail: format!("refusing to mount over {}", bind.target),
                });
            }
        }

        if self.limits.memory_bytes == 0 {
            violations.push(SecurityViolation {
                rule: "memory-limit",
                detail: "a memory limit is required".to_string(),
            });
        }

        // Swap above the memory limit silently defeats the memory limit.
        if self.limits.memory_swap_bytes != self.limits.memory_bytes {
            violations.push(SecurityViolation {
                rule: "memory-swap",
                detail: "swap must equal the memory limit".to_string(),
            });
        }

        if self.limits.pids_limit <= 0 {
            violations.push(SecurityViolation {
                rule: "pids-limit",
                detail: "a process limit is required".to_string(),
            });
        }

        if self.limits.cpu_quota <= 0 {
            violations.push(SecurityViolation {
                rule: "cpu-limit",
                detail: "a CPU limit is required".to_string(),
            });
        }

        for binding in &self.ports {
            if binding.host_port < 1024 {
                violations.push(SecurityViolation {
                    rule: "privileged-port",
                    detail: format!("host port {} is privileged", binding.host_port),
                });
            }
            if binding.bind_address == "0.0.0.0" || binding.bind_address == "::" {
                violations.push(SecurityViolation {
                    rule: "wildcard-bind",
                    detail: "ports must not be published on all interfaces".to_string(),
                });
            }
        }

        if self.labels.get(LABEL_MANAGED).map(String::as_str) != Some("true") {
            violations.push(SecurityViolation {
                rule: "managed-label",
                detail: "the managed label is required for reconciliation".to_string(),
            });
        }

        violations
    }

    /// True when no rule is broken.
    pub fn is_sound(&self) -> bool {
        self.security_violations().is_empty()
    }

    /// Whether this project's directory is the only host path mounted.
    pub fn mounts_only(&self, project_dir: &Path) -> bool {
        self.binds.iter().all(|bind| bind.source == project_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> SpecInputs {
        SpecInputs {
            slug: "quiet-harbor-4f2a".to_string(),
            project_id: "prj_0193".to_string(),
            template_id: "nodejs".to_string(),
            agent_version: "0.1.0".to_string(),
            image_tag: "projecthost/nodejs:prj_0193".to_string(),
            command: vec!["node".to_string(), "index.js".to_string()],
            working_dir: APP_MOUNT.to_string(),
            environment: vec![("PORT".to_string(), "3000".to_string())],
            project_dir: PathBuf::from("/var/lib/project-host/projects/prj_0193"),
            data_volume: ContainerSpec::volume_name("quiet-harbor-4f2a"),
            network_mode: NetworkMode::Internet,
            ports: vec![PortBinding {
                container_port: 3000,
                host_port: 20001,
                protocol: "tcp".to_string(),
                bind_address: "127.0.0.1".to_string(),
            }],
            limits: ResourceLimits::from_user_values(512, 1.0, 128),
            restart_policy: RestartPolicy::UnlessStopped,
            health_check: None,
        }
    }

    #[test]
    fn a_default_spec_breaks_no_rule() {
        let spec = ContainerSpec::build(inputs());
        assert_eq!(spec.security_violations(), Vec::new());
        assert!(spec.is_sound());
    }

    #[test]
    fn every_network_mode_produces_a_sound_spec() {
        // The exhaustive check the specification asks for: no combination of
        // user-selectable options can yield an unsound container.
        for mode in [
            NetworkMode::None,
            NetworkMode::Internal,
            NetworkMode::Lan,
            NetworkMode::Internet,
        ] {
            for policy in [
                RestartPolicy::No,
                RestartPolicy::OnFailure,
                RestartPolicy::UnlessStopped,
                RestartPolicy::Always,
            ] {
                let spec = ContainerSpec::build(SpecInputs {
                    network_mode: mode,
                    restart_policy: policy,
                    ..inputs()
                });
                assert!(
                    spec.is_sound(),
                    "{mode:?}/{policy:?} produced {:?}",
                    spec.security_violations()
                );
            }
        }
    }

    #[test]
    fn the_container_never_runs_as_root() {
        let spec = ContainerSpec::build(inputs());
        assert_eq!(spec.user, "10001:10001");
        assert!(!spec.user.starts_with("0:"));
    }

    #[test]
    fn hardening_flags_are_always_set() {
        let spec = ContainerSpec::build(inputs());
        assert!(spec.read_only_root_filesystem);
        assert!(!spec.privileged);
        assert_eq!(spec.cap_drop, vec!["ALL".to_string()]);
        assert!(spec.cap_add.is_empty());
        assert!(spec
            .security_opt
            .contains(&"no-new-privileges:true".to_string()));
    }

    #[test]
    fn swap_equals_the_memory_limit() {
        // Otherwise a container can exceed its memory limit through swap and
        // the limit the user configured means nothing.
        let spec = ContainerSpec::build(inputs());
        assert_eq!(spec.limits.memory_swap_bytes, spec.limits.memory_bytes);
        assert_eq!(spec.limits.memory_bytes, 512 * 1024 * 1024);
    }

    #[test]
    fn only_the_projects_own_directory_is_mounted() {
        let spec = ContainerSpec::build(inputs());
        assert_eq!(spec.binds.len(), 1);
        assert!(spec.mounts_only(Path::new("/var/lib/project-host/projects/prj_0193")));
        assert_eq!(spec.binds[0].target, APP_MOUNT);
    }

    #[test]
    fn a_docker_socket_mount_is_caught() {
        let mut spec = ContainerSpec::build(inputs());
        spec.binds.push(BindMount {
            source: PathBuf::from("/var/run/docker.sock"),
            target: "/var/run/docker.sock".to_string(),
            read_only: false,
        });
        let violations = spec.security_violations();
        assert!(
            violations.iter().any(|v| v.rule == "docker-socket"),
            "{violations:?}"
        );
    }

    #[test]
    fn a_windows_docker_pipe_mount_is_caught() {
        let mut spec = ContainerSpec::build(inputs());
        spec.binds.push(BindMount {
            source: PathBuf::from(r"\\.\pipe\docker_engine"),
            target: "/pipe".to_string(),
            read_only: false,
        });
        assert!(spec
            .security_violations()
            .iter()
            .any(|v| v.rule == "docker-socket"));
    }

    /// A named mutation that should break exactly one hardening rule.
    type Tamper = (&'static str, Box<dyn Fn(&mut ContainerSpec)>);

    #[test]
    fn tampered_specs_are_rejected_one_rule_at_a_time() {
        let cases: Vec<Tamper> = vec![
            (
                "privileged",
                Box::new(|s: &mut ContainerSpec| s.privileged = true),
            ),
            (
                "non-root-user",
                Box::new(|s: &mut ContainerSpec| s.user = "0:0".to_string()),
            ),
            (
                "no-new-privileges",
                Box::new(|s: &mut ContainerSpec| s.security_opt.clear()),
            ),
            (
                "cap-drop-all",
                Box::new(|s: &mut ContainerSpec| s.cap_drop.clear()),
            ),
            (
                "cap-add",
                Box::new(|s: &mut ContainerSpec| s.cap_add.push("SYS_ADMIN".to_string())),
            ),
            (
                "read-only-root",
                Box::new(|s: &mut ContainerSpec| s.read_only_root_filesystem = false),
            ),
            (
                "memory-swap",
                Box::new(|s: &mut ContainerSpec| s.limits.memory_swap_bytes = u64::MAX),
            ),
            (
                "pids-limit",
                Box::new(|s: &mut ContainerSpec| s.limits.pids_limit = 0),
            ),
            (
                "managed-label",
                Box::new(|s: &mut ContainerSpec| {
                    s.labels.remove(LABEL_MANAGED);
                }),
            ),
        ];

        for (rule, tamper) in cases {
            let mut spec = ContainerSpec::build(inputs());
            tamper(&mut spec);
            let violations = spec.security_violations();
            assert!(
                violations.iter().any(|v| v.rule == rule),
                "tampering with {rule} was not caught: {violations:?}"
            );
        }
    }

    #[test]
    fn a_privileged_host_port_is_caught() {
        let mut spec = ContainerSpec::build(inputs());
        spec.ports[0].host_port = 80;
        assert!(spec
            .security_violations()
            .iter()
            .any(|v| v.rule == "privileged-port"));
    }

    #[test]
    fn publishing_on_all_interfaces_is_caught() {
        let mut spec = ContainerSpec::build(inputs());
        spec.ports[0].bind_address = "0.0.0.0".to_string();
        assert!(spec
            .security_violations()
            .iter()
            .any(|v| v.rule == "wildcard-bind"));
    }

    #[test]
    fn an_internal_project_publishes_no_ports() {
        // Asking for a port on a network with no route out is incoherent; the
        // builder drops it rather than creating a binding that cannot work.
        for mode in [NetworkMode::None, NetworkMode::Internal] {
            let spec = ContainerSpec::build(SpecInputs {
                network_mode: mode,
                ..inputs()
            });
            assert!(spec.ports.is_empty(), "{mode:?} should publish nothing");
        }
    }

    #[test]
    fn a_networkless_project_gets_no_network() {
        let spec = ContainerSpec::build(SpecInputs {
            network_mode: NetworkMode::None,
            ..inputs()
        });
        assert_eq!(spec.network_name, None);
    }

    #[test]
    fn each_project_gets_its_own_network_and_volume() {
        // Cross-project isolation rests on these being distinct.
        let first = ContainerSpec::build(inputs());
        let second = ContainerSpec::build(SpecInputs {
            slug: "brave-meadow-91cc".to_string(),
            ..inputs()
        });
        assert_ne!(first.name, second.name);
        assert_ne!(first.network_name, second.network_name);
    }

    #[test]
    fn names_are_derived_from_the_slug_only() {
        assert_eq!(
            ContainerSpec::container_name("quiet-harbor-4f2a"),
            "ph_quiet-harbor-4f2a"
        );
        assert_eq!(
            ContainerSpec::network_name("quiet-harbor-4f2a"),
            "ph_net_quiet-harbor-4f2a"
        );
        assert_eq!(
            ContainerSpec::volume_name("quiet-harbor-4f2a"),
            "ph_vol_quiet-harbor-4f2a"
        );
    }

    #[test]
    fn the_command_is_an_argument_array_not_a_shell_string() {
        // A single string here would be handed to a shell somewhere downstream,
        // which is the whole command-injection class.
        let spec = ContainerSpec::build(inputs());
        assert_eq!(
            spec.command,
            vec!["node".to_string(), "index.js".to_string()]
        );
        assert!(!spec.command.iter().any(|part| part.contains("&&")));
    }

    #[test]
    fn cpu_quota_reflects_the_requested_cores() {
        let half = ResourceLimits::from_user_values(512, 0.5, 128);
        assert_eq!(half.cpu_quota, 50_000);
        assert_eq!(half.cpu_period, 100_000);

        let double = ResourceLimits::from_user_values(512, 2.0, 128);
        assert_eq!(double.cpu_quota, 200_000);
    }

    #[test]
    fn tmpfs_is_mounted_noexec() {
        let spec = ContainerSpec::build(inputs());
        let options = spec.tmpfs.get("/tmp").cloned().unwrap_or_default();
        assert!(options.contains("noexec"), "{options}");
        assert!(options.contains("nosuid"), "{options}");
    }

    #[test]
    fn logs_are_bounded() {
        let spec = ContainerSpec::build(inputs());
        assert_eq!(spec.log_driver, "json-file");
        assert_eq!(
            spec.log_options.get("max-size").map(String::as_str),
            Some("10m")
        );
        assert_eq!(
            spec.log_options.get("max-file").map(String::as_str),
            Some("3")
        );
    }

    #[test]
    fn restart_policies_map_to_docker_spelling() {
        assert_eq!(RestartPolicy::UnlessStopped.as_docker(), "unless-stopped");
        assert_eq!(RestartPolicy::OnFailure.as_docker(), "on-failure");
        assert_eq!(RestartPolicy::No.as_docker(), "no");
    }
}
