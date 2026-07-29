//! Managing and monitoring projects from Discord.
//!
//! A user invites the bot to their server and links it. Each project then gets
//! two channels: one carrying its logs and events, one carrying a control panel
//! of buttons and menus.
//!
//! # Trust
//!
//! Discord is the least trusted input surface in the application. The desktop
//! window is operated by the person who installed the software; a Discord
//! server may contain anyone the owner has ever invited, and the messages a
//! project emits are frequently influenced by strangers. Three consequences run
//! through every module here:
//!
//! * **Nothing arriving from Discord is an identifier until it has been
//!   validated.** A `custom_id` naming a project is parsed and re-checked
//!   ([`panel`]), never trusted because it came back from a message we sent.
//! * **Authorisation is decided when the button is pressed** ([`permissions`]),
//!   because a panel drawn today may be clicked next year by someone whose
//!   roles have since changed.
//! * **Project output is treated as hostile** ([`events`]): it cannot ping the
//!   server, cannot escape its code block, and cannot carry a secret value out.
//!
//! # What lives where
//!
//! | Module | Responsibility |
//! | ------ | -------------- |
//! | [`snowflake`] | Discord ids, and why they are strings on the wire |
//! | [`permissions`] | Who may do what, and when that is decided |
//! | [`panel`] | The control panel and its `custom_id` encoding |
//! | [`channels`] | Channel naming, and why a name is never an identifier |
//! | [`events`] | What is sent, and making it safe to send |
//!
//! Everything above is pure logic and fully tested. The gateway connection that
//! carries it to Discord needs a bot token and a network, and is therefore the
//! one part that cannot be verified by `cargo test`.

#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )
)]

pub mod channels;
pub mod events;
pub mod panel;
pub mod permissions;
pub mod snowflake;

pub use channels::{ChannelKind, NameTemplate, ProjectChannels, TemplateError};
pub use events::{
    format_event, format_log_batch, EventKind, NotificationSettings, OutgoingMessage, Redactor,
};
pub use panel::{Control, CustomIdError, PanelButton, PANEL_BUTTONS};
pub use permissions::{AccessPolicy, Action, Actor, Denied, Grant, Permission, Subject};
pub use snowflake::{Snowflake, SnowflakeError};
