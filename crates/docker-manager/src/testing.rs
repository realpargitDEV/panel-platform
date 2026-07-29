//! Test doubles. **Not compiled into a release binary.**
//!
//! This module exists only under `cfg(test)` or the explicit `testing` feature,
//! which no production dependency enables. Its purpose is to let `agent-core`
//! exercise Docker-dependent branches — the "Docker is down" banner, the
//! reconciler's degraded path — on a machine with no daemon.
//!
//! The separation is deliberate and load-bearing: the specification forbids
//! fake Docker responses, and the way to honour that while still testing those
//! branches is to make the fake impossible to reach outside a test build.

use crate::status::DockerStatus;
use crate::DockerProbe;

/// Returns whatever status it was constructed with.
#[derive(Debug, Clone)]
pub struct StubDockerProbe {
    status: DockerStatus,
}

impl StubDockerProbe {
    pub fn new(status: DockerStatus) -> Self {
        Self { status }
    }

    /// A daemon that is present and healthy.
    pub fn available() -> Self {
        Self::new(DockerStatus {
            available: true,
            version: Some("27.0.0".to_string()),
            api_version: Some("1.46".to_string()),
            endpoint_kind: Some("stub".to_string()),
            install_hint: None,
            error: None,
            containers_running: Some(0),
        })
    }

    /// A daemon that is not installed.
    pub fn unavailable() -> Self {
        Self::new(DockerStatus {
            available: false,
            version: None,
            api_version: None,
            endpoint_kind: None,
            install_hint: Some("Docker is not installed (test stub).".to_string()),
            error: None,
            containers_running: None,
        })
    }

    /// A daemon that answers the socket but not the API.
    pub fn degraded() -> Self {
        Self::new(DockerStatus::degraded(
            "stub".to_string(),
            "daemon did not respond (test stub)".to_string(),
        ))
    }
}

#[async_trait::async_trait]
impl DockerProbe for StubDockerProbe {
    async fn probe(&self) -> DockerStatus {
        self.status.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn the_stub_returns_what_it_was_given() {
        assert!(StubDockerProbe::available().probe().await.available);
        assert!(!StubDockerProbe::unavailable().probe().await.available);

        let degraded = StubDockerProbe::degraded().probe().await;
        assert!(!degraded.available);
        assert!(degraded.error.is_some());
    }

    #[tokio::test]
    async fn the_stub_labels_itself_so_it_cannot_be_mistaken_for_real() {
        let status = StubDockerProbe::unavailable().probe().await;
        assert!(
            status
                .install_hint
                .unwrap_or_default()
                .contains("test stub"),
            "a stub response must be identifiable in any log it reaches"
        );
    }
}
