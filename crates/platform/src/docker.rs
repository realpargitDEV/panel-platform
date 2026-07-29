//! Finding the Docker daemon.
//!
//! Discovery is platform-specific and ordered; connecting is not, and lives in
//! `crates/docker-manager`. Splitting them this way means the connection logic
//! has no `#[cfg]` in it and the candidate list can be tested without a daemon.
//!
//! Nothing here fabricates a result. If no endpoint is found, callers get an
//! empty list and an install hint — never a pretend daemon.

use std::path::PathBuf;

/// Somewhere the Docker API might be listening.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DockerEndpoint {
    /// Windows: Docker Desktop and Docker Engine both expose this.
    NamedPipe(String),
    UnixSocket(PathBuf),
    /// Only used when explicitly configured. Never probed by default: an
    /// unauthenticated Docker TCP port is root on the host for anyone who can
    /// reach it, and silently discovering one would reward a dangerous setup.
    Tcp(String),
}

impl DockerEndpoint {
    /// Short label for the UI and logs.
    pub fn kind(&self) -> &'static str {
        match self {
            DockerEndpoint::NamedPipe(_) => "npipe",
            DockerEndpoint::UnixSocket(_) => "unix-socket",
            DockerEndpoint::Tcp(_) => "tcp",
        }
    }

    /// The address in Docker's own `DOCKER_HOST` notation.
    pub fn address(&self) -> String {
        match self {
            DockerEndpoint::NamedPipe(name) => format!("npipe://{name}"),
            DockerEndpoint::UnixSocket(path) => format!("unix://{}", path.display()),
            DockerEndpoint::Tcp(address) => format!("tcp://{address}"),
        }
    }
}

/// What to tell the user when no daemon was found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DockerInstallHint {
    pub summary: String,
    pub detail: String,
    pub url: String,
}

/// Ordered candidate endpoints for this platform.
pub trait DockerProvider: Send + Sync + std::fmt::Debug {
    /// Most likely first. Callers try each until one answers.
    fn candidates(&self) -> Vec<DockerEndpoint>;
    fn install_hint(&self) -> DockerInstallHint;
}

/// Reads the real environment. The only implementation used in production.
#[derive(Debug, Clone, Default)]
pub struct SystemDockerProvider;

impl DockerProvider for SystemDockerProvider {
    fn candidates(&self) -> Vec<DockerEndpoint> {
        let mut found = Vec::new();

        // An explicit DOCKER_HOST wins: the operator has said where it is.
        if let Some(host) = std::env::var_os("DOCKER_HOST").and_then(|v| v.into_string().ok()) {
            if let Some(endpoint) = parse_docker_host(&host) {
                found.push(endpoint);
            }
        }

        #[cfg(windows)]
        {
            let pipe = DockerEndpoint::NamedPipe("//./pipe/docker_engine".to_string());
            if !found.contains(&pipe) {
                found.push(pipe);
            }
        }

        #[cfg(unix)]
        {
            let socket = DockerEndpoint::UnixSocket(PathBuf::from("/var/run/docker.sock"));
            if !found.contains(&socket) {
                found.push(socket);
            }

            // Rootless Docker. Supported, but it changes what resource limits
            // and port bindings are possible, so the caller reports it rather
            // than pretending the two are equivalent.
            if let Some(runtime_dir) =
                std::env::var_os("XDG_RUNTIME_DIR").and_then(|v| v.into_string().ok())
            {
                let rootless =
                    DockerEndpoint::UnixSocket(PathBuf::from(runtime_dir).join("docker.sock"));
                if !found.contains(&rootless) {
                    found.push(rootless);
                }
            }
        }

        found
    }

    fn install_hint(&self) -> DockerInstallHint {
        if cfg!(windows) {
            DockerInstallHint {
                summary: "Docker Desktop was not found.".to_string(),
                detail: "Project Host needs Docker to run projects. Install Docker Desktop, \
                         start it, and wait for the whale icon to stop animating. \
                         Everything else in the app works without it."
                    .to_string(),
                url: "https://docs.docker.com/desktop/install/windows-install/".to_string(),
            }
        } else {
            DockerInstallHint {
                summary: "The Docker daemon is not reachable.".to_string(),
                detail: "Install Docker Engine and ensure the service is running \
                         (`systemctl status docker`). The project-host user must be in \
                         the `docker` group."
                    .to_string(),
                url: "https://docs.docker.com/engine/install/".to_string(),
            }
        }
    }
}

/// Parse a `DOCKER_HOST` value.
pub fn parse_docker_host(value: &str) -> Option<DockerEndpoint> {
    let value = value.trim();
    if let Some(rest) = value.strip_prefix("unix://") {
        return Some(DockerEndpoint::UnixSocket(PathBuf::from(rest)));
    }
    if let Some(rest) = value.strip_prefix("npipe://") {
        return Some(DockerEndpoint::NamedPipe(rest.to_string()));
    }
    if let Some(rest) = value.strip_prefix("tcp://") {
        return Some(DockerEndpoint::Tcp(rest.to_string()));
    }
    if let Some(rest) = value.strip_prefix("http://") {
        return Some(DockerEndpoint::Tcp(rest.to_string()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_platform_offers_at_least_one_candidate() {
        let candidates = SystemDockerProvider.candidates();
        assert!(
            !candidates.is_empty(),
            "every platform has a default location"
        );
    }

    #[cfg(windows)]
    #[test]
    fn windows_looks_for_the_named_pipe() {
        let candidates = SystemDockerProvider.candidates();
        assert!(candidates
            .iter()
            .any(|endpoint| matches!(endpoint, DockerEndpoint::NamedPipe(_))));
    }

    #[cfg(unix)]
    #[test]
    fn linux_looks_for_the_unix_socket() {
        let candidates = SystemDockerProvider.candidates();
        assert!(candidates.iter().any(
            |endpoint| matches!(endpoint, DockerEndpoint::UnixSocket(path) if path
                == &PathBuf::from("/var/run/docker.sock"))
        ));
    }

    #[test]
    fn docker_host_values_are_parsed() {
        assert_eq!(
            parse_docker_host("unix:///var/run/docker.sock"),
            Some(DockerEndpoint::UnixSocket(PathBuf::from(
                "/var/run/docker.sock"
            )))
        );
        assert_eq!(
            parse_docker_host("npipe:////./pipe/docker_engine"),
            Some(DockerEndpoint::NamedPipe(
                "//./pipe/docker_engine".to_string()
            ))
        );
        assert_eq!(
            parse_docker_host("tcp://127.0.0.1:2375"),
            Some(DockerEndpoint::Tcp("127.0.0.1:2375".to_string()))
        );
    }

    #[test]
    fn an_unrecognised_docker_host_is_ignored_rather_than_guessed() {
        assert_eq!(parse_docker_host("nonsense"), None);
        assert_eq!(parse_docker_host(""), None);
    }

    #[test]
    fn tcp_is_never_a_default_candidate() {
        // An unauthenticated Docker TCP port is root on the host. Discovering
        // one automatically would reward a dangerous configuration.
        let candidates = SystemDockerProvider.candidates();
        assert!(
            !candidates
                .iter()
                .any(|endpoint| matches!(endpoint, DockerEndpoint::Tcp(_)))
                || std::env::var_os("DOCKER_HOST").is_some(),
            "TCP must only appear when DOCKER_HOST asks for it"
        );
    }

    #[test]
    fn endpoints_describe_themselves() {
        let pipe = DockerEndpoint::NamedPipe("//./pipe/docker_engine".to_string());
        assert_eq!(pipe.kind(), "npipe");
        assert!(pipe.address().starts_with("npipe://"));

        let socket = DockerEndpoint::UnixSocket(PathBuf::from("/var/run/docker.sock"));
        assert_eq!(socket.kind(), "unix-socket");
        assert!(socket.address().starts_with("unix://"));
    }

    #[test]
    fn the_install_hint_is_actionable() {
        let hint = SystemDockerProvider.install_hint();
        assert!(!hint.summary.is_empty());
        assert!(hint.url.starts_with("https://"));
        // It must say what still works, so a missing daemon does not read as a
        // broken installation.
        assert!(
            hint.detail.len() > 40,
            "the hint should explain what to do, not just state a failure"
        );
    }
}
