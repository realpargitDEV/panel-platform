//! The live connection to Discord.
//!
//! `project-host-discord` decides who may do what and what a message says, and
//! is pure: it can be reasoned about and tested without a network. This crate
//! is the other half — the part that needs a token, a socket and a running
//! Discord — and it is a separate crate so that the purity of the first one
//! stays a fact enforced by the dependency graph rather than a convention.
//!
//! # Shape
//!
//! | Module | Responsibility |
//! | ------ | -------------- |
//! | [`token`] | The bot token, in a wrapper that cannot print itself |
//! | [`error`] | What went wrong, and whether trying again could help |
//! | [`rest`] | The five REST calls, behind a trait a fake can implement |
//! | [`connection`] | One bot's live session, supervised like a project |
//! | [`supervisor`] | Every bot this installation is running, keyed by row id |
//!
//! # Testing
//!
//! Everything above [`rest::DiscordRest`] is driven by a fake in ordinary
//! `cargo test`, including the failure paths a real server produces rarely and
//! never on demand: rate limits, 5xx, and replies that are not JSON. What
//! cannot be covered that way is the socket itself, which is verified by hand
//! against a real bot.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod connection;
pub mod error;
pub mod rest;
pub mod supervisor;
pub mod token;

pub use connection::{start, ConnectionConfig, ConnectionHandle, ConnectionStatus, Observed};
pub use error::GatewayError;
pub use rest::{BotUser, CreatedChannel, DiscordRest, PartialGuild, PostedMessage, RestClient};
pub use supervisor::Supervisor;
pub use token::BotToken;
