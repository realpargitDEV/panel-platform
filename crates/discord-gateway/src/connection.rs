//! One bot's live connection, supervised the way a project is.
//!
//! The shape here deliberately mirrors `host-runner`'s supervisor: a config
//! goes in, [`start`] hands back a [`ConnectionHandle`], and the handle answers
//! [`ConnectionHandle::observe`] with what is true right now. A caller that can
//! drive a running project can drive a running bot without learning a second
//! vocabulary.
//!
//! # What this does not do
//!
//! It does not reconnect. `twilight-gateway` already owns that — sequence
//! numbers, the resume window, heartbeat timing, and the difference between a
//! close Discord expects us to recover from and one it does not. A second
//! reconnect loop wrapped around a library that already has one produces two
//! sockets racing to identify, which Discord answers by closing both. What this
//! module adds is the part twilight has no opinion about: how the connection
//! appears in a window, and when a human said stop.
//!
//! # Intents
//!
//! Only [`Intents::GUILDS`], which is not privileged. Posting a project's logs
//! to a channel needs no knowledge of message content or server members, so
//! asking for either would mean every user toggling two switches in the
//! developer portal to enable something the integration never reads.

use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use twilight_gateway::{
    CloseFrame, Config, Event, EventTypeFlags, Intents, Shard, ShardId, ShardState, StreamExt as _,
};

use crate::error::GatewayError;
use crate::token::BotToken;

/// Discord's close code for a token it would not accept.
const CLOSE_AUTHENTICATION_FAILED: u16 = 4004;

/// Discord's close code for an intent the application is not approved for.
const CLOSE_DISALLOWED_INTENTS: u16 = 4014;

/// How long [`ConnectionHandle::stop`] waits for a polite close before it stops
/// waiting.
///
/// A close frame that goes unanswered means the socket is already gone, and the
/// user pressed stop expecting the button to do something.
pub const DEFAULT_STOP_GRACE: Duration = Duration::from_secs(5);

/// What to connect as.
#[derive(Debug, Clone)]
pub struct ConnectionConfig {
    /// The bot this connection belongs to, as the database knows it.
    pub bot_row_id: String,
    pub token: BotToken,
}

/// What is true about a connection right now.
///
/// This crate has its own status words rather than the wire format's, for the
/// reason `host-runner` gives about `HostStatus`: it should be possible to
/// reason about a running connection without also holding the API shape in
/// mind. Translation happens one layer up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionStatus {
    /// Not running, because nobody has started it or somebody stopped it.
    Stopped,
    /// Reaching for a session: connecting, identifying, resuming, or waiting
    /// out a backoff between attempts.
    Connecting,
    /// Connected with a live session.
    Connected,
    /// Stopped and will not retry on its own.
    Failed,
}

impl ConnectionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ConnectionStatus::Stopped => "stopped",
            ConnectionStatus::Connecting => "connecting",
            ConnectionStatus::Connected => "connected",
            ConnectionStatus::Failed => "failed",
        }
    }
}

/// A snapshot of a connection.
#[derive(Debug, Clone)]
pub struct Observed {
    pub status: ConnectionStatus,
    /// Why it failed, in words safe to show a user. Never carries the token.
    pub failure_reason: Option<String>,
    /// How long the current session has been up.
    pub uptime: Option<Duration>,
    /// How many times twilight has re-established the session since start.
    pub reconnects: u32,
}

/// The mutable half, shared between the handle and its task.
#[derive(Debug)]
struct Shared {
    status: ConnectionStatus,
    failure_reason: Option<String>,
    connected_since: Option<Instant>,
    reconnects: u32,
    /// Set by [`ConnectionHandle::stop`], so the task can tell a close it asked
    /// for from one Discord initiated.
    stopping: bool,
}

/// A handle to one running connection.
#[derive(Debug, Clone)]
pub struct ConnectionHandle {
    bot_row_id: String,
    shared: Arc<Mutex<Shared>>,
    sender: twilight_gateway::MessageSender,
    task: Arc<tokio::task::JoinHandle<()>>,
}

impl ConnectionHandle {
    fn state(&self) -> MutexGuard<'_, Shared> {
        // A poisoned lock means a previous holder panicked while holding it.
        // The data behind it is four plain fields with no invariant spanning
        // them, so continuing with what is there beats refusing to report a
        // status. Same reasoning as `host-runner`'s supervisor.
        self.shared
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn bot_row_id(&self) -> &str {
        &self.bot_row_id
    }

    /// What is true right now.
    pub fn observe(&self) -> Observed {
        let state = self.state();
        Observed {
            status: state.status,
            failure_reason: state.failure_reason.clone(),
            uptime: state.connected_since.map(|since| since.elapsed()),
            reconnects: state.reconnects,
        }
    }

    /// Ask Discord to close the session, and stop waiting after `grace`.
    ///
    /// Closing politely rather than dropping the socket matters: a clean close
    /// tells Discord the session is finished, where an abandoned one leaves it
    /// resumable and the bot showing as online for a while afterwards.
    pub async fn stop(&self, grace: Duration) {
        {
            let mut state = self.state();
            if state.status == ConnectionStatus::Stopped {
                return;
            }
            state.stopping = true;
        }

        // Fails only when the shard is already gone, which is the outcome the
        // caller wanted.
        let _ = self.sender.close(CloseFrame::NORMAL);

        // The task ends on its own once the close is acknowledged. Aborting
        // after the grace covers the case where it never is.
        if tokio::time::timeout(grace, wait_for(&self.task))
            .await
            .is_err()
        {
            self.task.abort();
        }

        let mut state = self.state();
        state.status = ConnectionStatus::Stopped;
        state.connected_since = None;
    }
}

/// Wait for a task without owning its handle.
async fn wait_for(task: &tokio::task::JoinHandle<()>) {
    // `JoinHandle` needs `&mut` to be awaited, which a shared handle cannot
    // give. Polling its `is_finished` is enough here: the caller only needs to
    // know when to stop waiting, and the interval is short next to the grace.
    while !task.is_finished() {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Connect, and keep connecting.
///
/// Returns as soon as the task is spawned rather than when the session is
/// established: a window that blocks until Discord answers is a window that
/// hangs on a bad network, and the status is observable from the moment this
/// returns.
pub fn start(config: ConnectionConfig) -> ConnectionHandle {
    let shared = Arc::new(Mutex::new(Shared {
        status: ConnectionStatus::Connecting,
        failure_reason: None,
        connected_since: None,
        reconnects: 0,
        stopping: false,
    }));

    let shard_config = Config::new(config.token.expose().to_string(), Intents::GUILDS);
    let shard = Shard::with_config(ShardId::ONE, shard_config);
    let sender = shard.sender();
    let task = tokio::spawn(run(shard, Arc::clone(&shared)));

    ConnectionHandle {
        bot_row_id: config.bot_row_id,
        shared,
        sender,
        task: Arc::new(task),
    }
}

/// The event loop.
///
/// Slice one observes rather than acts: the connection proves itself by
/// reaching `Connected` and staying there. Dispatching interactions is the next
/// piece and hangs off the same loop.
async fn run(mut shard: Shard, shared: Arc<Mutex<Shared>>) {
    let mut was_connected = false;

    while let Some(item) = shard.next_event(EventTypeFlags::all()).await {
        match item {
            Ok(Event::GatewayClose(frame)) => {
                if let Some(error) = fatal_close(frame.as_ref()) {
                    record_failure(&shared, &error);
                    return;
                }
            }
            Ok(_) => {}
            Err(source) => {
                // A receive error is not by itself the end: twilight reconnects
                // under us, and the state check below is what decides whether
                // this connection is still viable.
                tracing::warn!(error = %source, "error receiving a gateway event");
            }
        }

        {
            let mut state = lock(&shared);
            if state.stopping {
                state.status = ConnectionStatus::Stopped;
                state.connected_since = None;
                return;
            }

            match shard.state() {
                ShardState::Active => {
                    if !was_connected {
                        was_connected = true;
                        state.connected_since = Some(Instant::now());
                    }
                    state.status = ConnectionStatus::Connected;
                    state.failure_reason = None;
                }
                ShardState::Identifying | ShardState::Resuming => {
                    state.status = ConnectionStatus::Connecting;
                }
                ShardState::Disconnected { .. } => {
                    if was_connected {
                        was_connected = false;
                        state.reconnects = state.reconnects.saturating_add(1);
                        state.connected_since = None;
                    }
                    state.status = ConnectionStatus::Connecting;
                }
                ShardState::FatallyClosed => {
                    state.status = ConnectionStatus::Failed;
                    if state.failure_reason.is_none() {
                        state.failure_reason = Some(
                            GatewayError::Fatal("Discord closed the session".into()).user_facing(),
                        );
                    }
                    return;
                }
            }
        }
    }

    // The stream ended. Either somebody asked for that, or the connection is
    // over and twilight has stopped trying.
    let mut state = lock(&shared);
    if state.stopping || state.status == ConnectionStatus::Stopped {
        state.status = ConnectionStatus::Stopped;
    } else if state.status != ConnectionStatus::Failed {
        state.status = ConnectionStatus::Failed;
        state.failure_reason.get_or_insert_with(|| {
            GatewayError::Disconnected("the gateway stream ended".into()).user_facing()
        });
    }
    state.connected_since = None;
}

fn lock(shared: &Arc<Mutex<Shared>>) -> MutexGuard<'_, Shared> {
    shared
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn record_failure(shared: &Arc<Mutex<Shared>>, error: &GatewayError) {
    let mut state = lock(shared);
    state.status = ConnectionStatus::Failed;
    state.failure_reason = Some(error.user_facing());
    state.connected_since = None;
}

/// Whether a close frame means "do not try again".
///
/// Only the two codes a user can actually fix are named. Everything else is
/// left to twilight, which knows which of the remaining codes are resumable —
/// duplicating that table here would mean maintaining a second copy of it.
fn fatal_close(frame: Option<&CloseFrame<'_>>) -> Option<GatewayError> {
    match frame?.code {
        CLOSE_AUTHENTICATION_FAILED => Some(GatewayError::InvalidToken),
        CLOSE_DISALLOWED_INTENTS => Some(GatewayError::DisallowedIntents),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_token_is_recognised_as_fatal() {
        let frame = CloseFrame::new(CLOSE_AUTHENTICATION_FAILED, "Authentication failed.");
        assert!(matches!(
            fatal_close(Some(&frame)),
            Some(GatewayError::InvalidToken)
        ));
    }

    #[test]
    fn a_missing_intent_is_recognised_as_fatal() {
        let frame = CloseFrame::new(CLOSE_DISALLOWED_INTENTS, "Disallowed intents.");
        assert!(matches!(
            fatal_close(Some(&frame)),
            Some(GatewayError::DisallowedIntents)
        ));
    }

    #[test]
    fn an_ordinary_close_is_left_to_the_library_to_recover_from() {
        // 1001 is a going-away, which is resumable. Treating it as fatal would
        // strand a connection that would have come back on its own.
        let frame = CloseFrame::new(1001, "Going away.");
        assert!(fatal_close(Some(&frame)).is_none());
        assert!(fatal_close(None).is_none());
    }

    #[test]
    fn status_words_are_stable() {
        // They reach the window and a stored row; renaming one silently is a
        // status that stops matching what is displayed.
        assert_eq!(ConnectionStatus::Stopped.as_str(), "stopped");
        assert_eq!(ConnectionStatus::Connecting.as_str(), "connecting");
        assert_eq!(ConnectionStatus::Connected.as_str(), "connected");
        assert_eq!(ConnectionStatus::Failed.as_str(), "failed");
    }

    #[test]
    fn a_failure_reason_never_contains_the_token() {
        let reason = GatewayError::InvalidToken.user_facing();
        assert!(!reason.contains("Bot "));
        assert!(reason.contains("developer portal"));
    }
}
