//! The handful of REST calls this integration makes.
//!
//! Discord's HTTP API is large and this uses five endpoints, which is why it is
//! written here rather than taken from a client library. The parts worth owning
//! are the ones a general client would hide:
//!
//! * **429 is normal.** Discord rate-limits per route and expects the client to
//!   wait exactly as long as it says. [`RestClient`] honours `retry_after`
//!   rather than applying a backoff of its own invention, because a guessed
//!   delay is either too short — which earns another 429 and, repeated, a
//!   temporary ban — or needlessly long.
//! * **The reply is not trusted to be JSON.** A proxy, a captive portal or a
//!   Cloudflare error page all answer with something that is not the documented
//!   body, and the failure should say so rather than surfacing a parse error.
//!
//! Everything above the [`DiscordRest`] trait is testable without a network,
//! which is the point of the trait: the reconnect and provisioning logic is
//! driven by a fake in ordinary `cargo test`.

use std::future::Future;
use std::time::Duration;

use serde::Deserialize;

use crate::error::GatewayError;
use crate::token::BotToken;

/// Discord's API base. Versioned, because an unversioned URL silently follows
/// Discord's default and would change behaviour under us.
pub const API_BASE: &str = "https://discord.com/api/v10";

/// How many times a rate-limited or server-faulted request is retried.
///
/// Three is enough to ride out a burst and short enough that a genuinely broken
/// route reports rather than hangs.
const MAX_ATTEMPTS: u32 = 3;

/// Refuse to sit on a rate limit longer than this.
///
/// Discord occasionally answers a global limit with a very long `retry_after`.
/// Waiting it out inside a request would look like a hung application; failing
/// with a retryable error hands the decision to the supervisor, which has a
/// status the window can show.
const MAX_RATE_LIMIT_WAIT: Duration = Duration::from_secs(30);

/// Who the token belongs to.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct BotUser {
    pub id: String,
    pub username: String,
}

/// A server the bot is a member of.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PartialGuild {
    pub id: String,
    pub name: String,
}

/// A channel, as Discord returns it.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct CreatedChannel {
    pub id: String,
    pub name: String,
}

/// A posted message.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct PostedMessage {
    pub id: String,
}

/// The REST calls the integration needs.
///
/// A trait so the layers above it can be tested against a fake that returns
/// 429s, 5xx and malformed bodies on demand — failure modes a real server
/// produces rarely and never on request.
pub trait DiscordRest: Send + Sync {
    /// Who this token belongs to. Also the cheapest way to find out whether a
    /// token is valid at all, which is what the "add a bot" flow needs.
    fn current_user(
        &self,
        token: &BotToken,
    ) -> impl Future<Output = Result<BotUser, GatewayError>> + Send;

    /// The servers this bot has been invited to.
    fn current_user_guilds(
        &self,
        token: &BotToken,
    ) -> impl Future<Output = Result<Vec<PartialGuild>, GatewayError>> + Send;

    /// Create a text channel in a server.
    fn create_text_channel(
        &self,
        token: &BotToken,
        guild_id: &str,
        name: &str,
    ) -> impl Future<Output = Result<CreatedChannel, GatewayError>> + Send;

    /// Post a message to a channel.
    fn post_message(
        &self,
        token: &BotToken,
        channel_id: &str,
        content: &str,
    ) -> impl Future<Output = Result<PostedMessage, GatewayError>> + Send;

    /// Replace the content of a message the bot posted earlier.
    fn edit_message(
        &self,
        token: &BotToken,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> impl Future<Output = Result<PostedMessage, GatewayError>> + Send;
}

/// The real client.
#[derive(Debug, Clone)]
pub struct RestClient {
    http: reqwest::Client,
    base: String,
}

impl RestClient {
    /// Build a client.
    ///
    /// The user agent is required by Discord and they do enforce it; a request
    /// without one is refused.
    pub fn new() -> Result<Self, GatewayError> {
        let http = reqwest::Client::builder()
            .user_agent(concat!(
                "DiscordBot (https://github.com/paar-git/panel-platform, ",
                env!("CARGO_PKG_VERSION"),
                ")"
            ))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|error| GatewayError::Http(error.to_string()))?;

        Ok(Self {
            http,
            base: API_BASE.to_string(),
        })
    }

    /// Point the client at a different base URL, for tests against a local
    /// server.
    pub fn with_base(mut self, base: impl Into<String>) -> Self {
        self.base = base.into();
        self
    }

    /// Send a request, waiting out rate limits, and decode the reply.
    async fn send<T: serde::de::DeserializeOwned>(
        &self,
        token: &BotToken,
        method: reqwest::Method,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> Result<T, GatewayError> {
        let url = format!("{}{path}", self.base);
        let mut attempt = 0;

        loop {
            attempt += 1;

            let mut request = self
                .http
                .request(method.clone(), &url)
                .header(reqwest::header::AUTHORIZATION, token.header_value());
            if let Some(ref body) = body {
                request = request.json(body);
            }

            let response = request
                .send()
                .await
                .map_err(|error| GatewayError::Http(error.to_string()))?;

            let status = response.status();

            if status == reqwest::StatusCode::UNAUTHORIZED {
                return Err(GatewayError::InvalidToken);
            }

            if status == reqwest::StatusCode::TOO_MANY_REQUESTS && attempt < MAX_ATTEMPTS {
                match retry_after(&response) {
                    Some(wait) if wait <= MAX_RATE_LIMIT_WAIT => {
                        tokio::time::sleep(wait).await;
                        continue;
                    }
                    _ => {}
                }
            }

            if status.is_server_error() && attempt < MAX_ATTEMPTS {
                // Discord's own guidance for 5xx, and unlike a 429 there is no
                // header saying how long to wait.
                tokio::time::sleep(Duration::from_millis(500 * u64::from(attempt))).await;
                continue;
            }

            let text = response
                .text()
                .await
                .map_err(|error| GatewayError::Http(error.to_string()))?;

            if !status.is_success() {
                return Err(GatewayError::Api {
                    status: status.as_u16(),
                    body: truncate(&text),
                });
            }

            return serde_json::from_str(&text)
                .map_err(|error| GatewayError::MalformedReply(error.to_string()));
        }
    }
}

/// How long Discord asked us to wait.
///
/// The header is seconds as a decimal. `Retry-After` is read in preference to
/// the JSON body because it is present on both the per-route and global cases.
fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    let raw = response
        .headers()
        .get("retry-after")
        .or_else(|| response.headers().get("x-ratelimit-reset-after"))?
        .to_str()
        .ok()?;

    let seconds: f64 = raw.parse().ok()?;
    if !seconds.is_finite() || seconds < 0.0 {
        return None;
    }
    Some(Duration::from_secs_f64(seconds))
}

/// Keep an error body short enough to log.
///
/// Discord's errors are small; a page-long body means something else answered,
/// and the first part of it is the useful part.
fn truncate(text: &str) -> String {
    const LIMIT: usize = 500;
    if text.chars().count() <= LIMIT {
        return text.to_string();
    }
    text.chars().take(LIMIT).collect::<String>() + "…"
}

impl DiscordRest for RestClient {
    async fn current_user(&self, token: &BotToken) -> Result<BotUser, GatewayError> {
        self.send(token, reqwest::Method::GET, "/users/@me", None)
            .await
    }

    async fn current_user_guilds(
        &self,
        token: &BotToken,
    ) -> Result<Vec<PartialGuild>, GatewayError> {
        self.send(token, reqwest::Method::GET, "/users/@me/guilds", None)
            .await
    }

    async fn create_text_channel(
        &self,
        token: &BotToken,
        guild_id: &str,
        name: &str,
    ) -> Result<CreatedChannel, GatewayError> {
        // Type 0 is a guild text channel.
        self.send(
            token,
            reqwest::Method::POST,
            &format!("/guilds/{guild_id}/channels"),
            Some(serde_json::json!({ "name": name, "type": 0 })),
        )
        .await
    }

    async fn post_message(
        &self,
        token: &BotToken,
        channel_id: &str,
        content: &str,
    ) -> Result<PostedMessage, GatewayError> {
        // `allowed_mentions` empty is the second line behind the neutralising
        // that `discord::events` already does: even if a mention survived
        // formatting, Discord is told not to resolve it.
        self.send(
            token,
            reqwest::Method::POST,
            &format!("/channels/{channel_id}/messages"),
            Some(serde_json::json!({
                "content": content,
                "allowed_mentions": { "parse": [] },
            })),
        )
        .await
    }

    async fn edit_message(
        &self,
        token: &BotToken,
        channel_id: &str,
        message_id: &str,
        content: &str,
    ) -> Result<PostedMessage, GatewayError> {
        self.send(
            token,
            reqwest::Method::PATCH,
            &format!("/channels/{channel_id}/messages/{message_id}"),
            Some(serde_json::json!({
                "content": content,
                "allowed_mentions": { "parse": [] },
            })),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_error_body_is_shortened() {
        let long = "x".repeat(2000);
        let shortened = truncate(&long);

        assert!(shortened.chars().count() <= 501);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn a_short_error_body_is_left_alone() {
        assert_eq!(truncate("missing access"), "missing access");
    }

    #[test]
    fn truncation_counts_characters_not_bytes() {
        // Slicing by byte index here would panic mid-codepoint, which is
        // exactly the sort of thing an error path must not do.
        let text = "é".repeat(600);
        let shortened = truncate(&text);
        assert_eq!(shortened.chars().count(), 501);
    }
}
