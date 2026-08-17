//! What can go wrong on the way to Discord, and which failures are worth
//! retrying.
//!
//! The distinction this module exists to draw is between a connection that is
//! *down* and one that is *finished*. A network that dropped, a gateway that
//! asked us to reconnect, a 502 from the API — those are weather, and the
//! answer is to wait and try again. A token Discord rejected, or an intent the
//! application is not approved for, will fail identically forever; retrying
//! those produces a connection that flaps until someone reads the log, and in
//! the token case can get the bot rate-limited for repeated bad IDENTIFYs.

/// Something that stopped a connection.
#[derive(Debug, thiserror::Error)]
pub enum GatewayError {
    #[error("Discord rejected the bot token")]
    InvalidToken,

    #[error("the bot is not approved for the intents it asked for")]
    DisallowedIntents,

    #[error("Discord closed the connection and will not accept a reconnect: {0}")]
    Fatal(String),

    #[error("the connection to Discord dropped: {0}")]
    Disconnected(String),

    #[error("could not reach Discord: {0}")]
    Http(String),

    #[error("Discord answered {status}: {body}")]
    Api { status: u16, body: String },

    #[error("Discord's reply could not be read: {0}")]
    MalformedReply(String),

    #[error("no bot with id {0}")]
    NoSuchBot(String),

    #[error("that bot is already connected")]
    AlreadyRunning,
}

impl GatewayError {
    /// Whether waiting and trying again could produce a different outcome.
    ///
    /// The default for an unrecognised failure is `true`. Getting this wrong in
    /// the retryable direction costs a wasted reconnect that backoff already
    /// bounds; getting it wrong the other way silently strands a connection
    /// that would have recovered on its own.
    pub fn is_retryable(&self) -> bool {
        match self {
            GatewayError::InvalidToken
            | GatewayError::DisallowedIntents
            | GatewayError::Fatal(_)
            | GatewayError::NoSuchBot(_)
            | GatewayError::AlreadyRunning => false,

            GatewayError::Disconnected(_) | GatewayError::Http(_) => true,

            // 429 is the case this arm exists for: it is the API asking for
            // patience, not refusing the request. 5xx is Discord having a bad
            // day. 4xx otherwise means this request will never be accepted as
            // written.
            GatewayError::Api { status, .. } => *status == 429 || *status >= 500,

            GatewayError::MalformedReply(_) => true,
        }
    }
}

/// What a caller should be told, with nothing secret in it.
///
/// A token that failed must not reach a log line or a Discord channel, and the
/// simplest way to guarantee that is for the type that formats failures to have
/// no arm that can carry one.
impl GatewayError {
    pub fn user_facing(&self) -> String {
        match self {
            GatewayError::InvalidToken => {
                "Discord rejected this token. It may have been reset in the developer portal."
                    .to_string()
            }
            GatewayError::DisallowedIntents => {
                "This bot is missing a privileged intent. Enable Message Content and Server \
                 Members in the developer portal, then start it again."
                    .to_string()
            }
            other => other.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_rejected_token_is_not_retried() {
        assert!(!GatewayError::InvalidToken.is_retryable());
        assert!(!GatewayError::DisallowedIntents.is_retryable());
    }

    #[test]
    fn a_dropped_connection_is_retried() {
        assert!(GatewayError::Disconnected("reset by peer".into()).is_retryable());
        assert!(GatewayError::Http("dns failure".into()).is_retryable());
    }

    #[test]
    fn rate_limits_and_server_faults_are_retried_but_bad_requests_are_not() {
        let retryable = [429, 500, 502, 503];
        for status in retryable {
            assert!(
                GatewayError::Api {
                    status,
                    body: String::new()
                }
                .is_retryable(),
                "{status} should be retryable"
            );
        }

        let permanent = [400, 401, 403, 404];
        for status in permanent {
            assert!(
                !GatewayError::Api {
                    status,
                    body: String::new()
                }
                .is_retryable(),
                "{status} should not be retryable"
            );
        }
    }

    #[test]
    fn the_token_never_appears_in_a_user_facing_message() {
        let message = GatewayError::InvalidToken.user_facing();
        assert!(message.contains("developer portal"));
        assert!(!message.contains("token="));
    }
}
