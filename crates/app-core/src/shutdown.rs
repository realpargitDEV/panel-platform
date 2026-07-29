//! Shutdown signalling.
//!
//! One `watch` channel, so every task learns about shutdown at once and the
//! application can be stopped by a signal, by the window closing, or by a
//! test — all through the same path, which means the path is exercised
//! constantly rather than only in production.

use tokio::sync::watch;

/// Fires once. Cloning the receiver is how tasks subscribe.
#[derive(Debug, Clone)]
pub struct Shutdown {
    sender: watch::Sender<bool>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    pub fn new() -> Self {
        let (sender, _) = watch::channel(false);
        Self { sender }
    }

    pub fn subscribe(&self) -> watch::Receiver<bool> {
        self.sender.subscribe()
    }

    /// Request shutdown. Safe to call more than once.
    ///
    /// `send_replace` rather than `send`: `send` returns an error and leaves
    /// the value unchanged when there are no receivers, so a shutdown
    /// requested before any task subscribed would be silently lost — and a
    /// task subscribing afterwards would wait forever for an event that had
    /// already been requested.
    pub fn trigger(&self) {
        self.sender.send_replace(true);
    }

    pub fn is_triggered(&self) -> bool {
        *self.sender.borrow()
    }
}

/// Resolve when the operating system asks the process to stop.
///
/// Ctrl-C on both platforms, plus SIGTERM on Unix — systemd sends SIGTERM, and
/// a process that ignored it would be killed after the stop timeout instead of
/// checkpointing its database.
pub async fn wait_for_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(stream) => stream,
            Err(error) => {
                tracing::warn!(%error, "could not listen for SIGTERM; Ctrl-C only");
                let _ = tokio::signal::ctrl_c().await;
                return;
            }
        };

        tokio::select! {
            _ = tokio::signal::ctrl_c() => tracing::info!("received Ctrl-C"),
            _ = terminate.recv() => tracing::info!("received SIGTERM"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("received Ctrl-C");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribers_see_the_trigger() {
        let shutdown = Shutdown::new();
        let mut receiver = shutdown.subscribe();
        assert!(!shutdown.is_triggered());

        shutdown.trigger();
        receiver.changed().await.expect("changed");
        assert!(*receiver.borrow());
        assert!(shutdown.is_triggered());
    }

    #[tokio::test]
    async fn every_subscriber_is_notified() {
        let shutdown = Shutdown::new();
        let mut first = shutdown.subscribe();
        let mut second = shutdown.subscribe();

        shutdown.trigger();
        first.changed().await.expect("first");
        second.changed().await.expect("second");
    }

    #[tokio::test]
    async fn triggering_twice_is_harmless() {
        let shutdown = Shutdown::new();
        shutdown.trigger();
        shutdown.trigger();
        assert!(shutdown.is_triggered());
    }

    #[tokio::test]
    async fn a_late_subscriber_sees_the_current_value() {
        // A task started during shutdown must not wait forever for an event
        // that has already happened.
        let shutdown = Shutdown::new();
        shutdown.trigger();
        let receiver = shutdown.subscribe();
        assert!(*receiver.borrow());
    }
}
