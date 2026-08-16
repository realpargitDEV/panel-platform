//! Every bot this installation is running, and the ones it is not.
//!
//! The unit of control is a bot, not the integration: starting one connection
//! must not disturb another, and one bot with a revoked token must not stop the
//! rest from running. That is the whole reason this is a map rather than an
//! `Option`.
//!
//! # Why absence means stopped
//!
//! A bot with no entry in the map is reported as [`ConnectionStatus::Stopped`]
//! rather than as an error. The window lists bots from the database and asks
//! this type about each one; a bot that has never been started is the ordinary
//! case, not a lookup failure. Failures that matter — a rejected token — are
//! held in the entry, so they survive being looked at.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::connection::{
    self, ConnectionConfig, ConnectionHandle, ConnectionStatus, Observed, DEFAULT_STOP_GRACE,
};
use crate::error::GatewayError;

/// The running connections, keyed by the bot's database row id.
#[derive(Debug, Clone, Default)]
pub struct Supervisor {
    running: Arc<Mutex<BTreeMap<String, ConnectionHandle>>>,
}

impl Supervisor {
    pub fn new() -> Self {
        Self::default()
    }

    fn state(&self) -> MutexGuard<'_, BTreeMap<String, ConnectionHandle>> {
        self.running
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Start a bot's connection.
    ///
    /// Starting one that is already running is refused rather than silently
    /// ignored. Two sessions identifying as the same bot is a state Discord
    /// resolves by closing one of them, so a caller that has lost track of what
    /// it started should hear about it.
    ///
    /// A connection that previously *failed* is replaced, not refused: pressing
    /// start after fixing a token is exactly how a user retries.
    pub fn start(&self, config: ConnectionConfig) -> Result<(), GatewayError> {
        if config.token.is_empty() {
            return Err(GatewayError::InvalidToken);
        }

        let mut running = self.state();

        if let Some(existing) = running.get(&config.bot_row_id) {
            let status = existing.observe().status;
            if status == ConnectionStatus::Connected || status == ConnectionStatus::Connecting {
                return Err(GatewayError::AlreadyRunning);
            }
        }

        let bot_row_id = config.bot_row_id.clone();
        running.insert(bot_row_id, connection::start(config));
        Ok(())
    }

    /// Stop a bot's connection, if it has one.
    ///
    /// Stopping a bot that is not running is success. The caller asked for it
    /// to not be running, and it is not.
    pub async fn stop(&self, bot_row_id: &str) {
        // Taken out of the map before awaiting: holding a std `Mutex` across an
        // await would make this future non-`Send`, and the guard has no reason
        // to outlive the removal.
        let handle = self.state().remove(bot_row_id);

        if let Some(handle) = handle {
            handle.stop(DEFAULT_STOP_GRACE).await;
        }
    }

    /// Stop everything, for application shutdown.
    ///
    /// Sequential rather than concurrent: the count is small, and a clean close
    /// per connection matters more here than the milliseconds saved.
    pub async fn stop_all(&self) {
        let handles: Vec<ConnectionHandle> = {
            let mut running = self.state();
            std::mem::take(&mut *running).into_values().collect()
        };

        for handle in handles {
            handle.stop(DEFAULT_STOP_GRACE).await;
        }
    }

    /// What one bot is doing.
    pub fn observe(&self, bot_row_id: &str) -> Observed {
        self.state()
            .get(bot_row_id)
            .map(ConnectionHandle::observe)
            .unwrap_or_else(stopped)
    }

    /// What every started bot is doing, keyed by row id.
    pub fn observe_all(&self) -> BTreeMap<String, Observed> {
        self.state()
            .iter()
            .map(|(id, handle)| (id.clone(), handle.observe()))
            .collect()
    }

    /// How many connections are currently established.
    ///
    /// What the window's "N running" counter shows, which is why it counts
    /// `Connected` rather than map entries: a bot that is retrying is not
    /// running yet.
    pub fn connected_count(&self) -> usize {
        self.state()
            .values()
            .filter(|handle| handle.observe().status == ConnectionStatus::Connected)
            .count()
    }

    pub fn is_running(&self, bot_row_id: &str) -> bool {
        matches!(
            self.observe(bot_row_id).status,
            ConnectionStatus::Connected | ConnectionStatus::Connecting
        )
    }
}

/// The answer for a bot that was never started.
fn stopped() -> Observed {
    Observed {
        status: ConnectionStatus::Stopped,
        failure_reason: None,
        uptime: None,
        reconnects: 0,
    }
}

/// The grace each connection is given at shutdown, re-exported so a caller
/// choosing its own shutdown budget can see what it is buying.
pub const SHUTDOWN_GRACE: Duration = DEFAULT_STOP_GRACE;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::token::BotToken;

    fn config(id: &str, token: &str) -> ConnectionConfig {
        ConnectionConfig {
            bot_row_id: id.to_string(),
            token: BotToken::new(token),
        }
    }

    #[test]
    fn a_bot_that_was_never_started_is_stopped_not_missing() {
        let supervisor = Supervisor::new();
        let observed = supervisor.observe("bot_unknown");

        assert_eq!(observed.status, ConnectionStatus::Stopped);
        assert!(observed.failure_reason.is_none());
        assert!(!supervisor.is_running("bot_unknown"));
    }

    #[test]
    fn an_empty_token_is_refused_before_a_socket_is_opened() {
        // Reaching Discord to be told a blank token is invalid wastes an
        // IDENTIFY, and repeated bad IDENTIFYs are what get a bot banned.
        let supervisor = Supervisor::new();
        let error = supervisor
            .start(config("bot_a", "   "))
            .expect_err("a blank token should not be dialled");

        assert!(matches!(error, GatewayError::InvalidToken));
        assert_eq!(
            supervisor.observe("bot_a").status,
            ConnectionStatus::Stopped
        );
    }

    #[tokio::test]
    async fn starting_the_same_bot_twice_is_refused() {
        // Two sessions identifying as one bot is a state Discord resolves by
        // closing one of them.
        let supervisor = Supervisor::new();
        supervisor.start(config("bot_a", "a-token")).expect("first");

        let error = supervisor
            .start(config("bot_a", "a-token"))
            .expect_err("the second start should be refused");
        assert!(matches!(error, GatewayError::AlreadyRunning));

        supervisor.stop_all().await;
    }

    #[tokio::test]
    async fn bots_are_independent_of_one_another() {
        // The point of the map: stopping one must not disturb the other.
        let supervisor = Supervisor::new();
        supervisor.start(config("bot_a", "a-token")).expect("a");
        supervisor.start(config("bot_b", "b-token")).expect("b");
        assert_eq!(supervisor.observe_all().len(), 2);

        supervisor.stop("bot_a").await;

        assert_eq!(
            supervisor.observe("bot_a").status,
            ConnectionStatus::Stopped
        );
        assert!(
            supervisor.observe_all().contains_key("bot_b"),
            "stopping one bot removed the other"
        );

        supervisor.stop_all().await;
        assert!(supervisor.observe_all().is_empty());
    }

    #[tokio::test]
    async fn stopping_a_bot_that_is_not_running_is_success() {
        let supervisor = Supervisor::new();
        supervisor.stop("bot_never_started").await;
        assert_eq!(
            supervisor.observe("bot_never_started").status,
            ConnectionStatus::Stopped
        );
    }

    #[tokio::test]
    async fn a_started_bot_can_be_started_again_after_being_stopped() {
        // Stop then start is the ordinary way a user restarts a connection.
        let supervisor = Supervisor::new();
        supervisor.start(config("bot_a", "a-token")).expect("first");
        supervisor.stop("bot_a").await;

        supervisor
            .start(config("bot_a", "a-token"))
            .expect("restarting a stopped bot should be allowed");

        supervisor.stop_all().await;
    }
}
