//! Who in a Discord server may do what.
//!
//! Three rules shape this module, and all three exist because Discord is a
//! hostile input surface in a way the desktop window is not:
//!
//! 1. **Default deny.** A member with no matching grant gets nothing. Being in
//!    the server is not permission to stop a production bot.
//! 2. **Authorisation happens when the button is pressed, never when it is
//!    drawn.** A control panel message sits in a channel for months. The roles
//!    of the person who eventually clicks it are whatever they are at that
//!    moment, so [`AccessPolicy::authorise`] is called from the interaction
//!    handler with the roles Discord attached to *that* interaction.
//! 3. **Nobody can lock themselves out.** The account that linked the server
//!    keeps administrative access no matter what the grants say.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::snowflake::Snowflake;

/// What a member is allowed to do, in increasing order.
///
/// The ordering is the authorisation check: a required level is satisfied by
/// any level greater than or equal to it, which `PartialOrd` gives us for free
/// and which a set of booleans would not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// Read-only: status, resource usage, recent errors, logs.
    View,
    /// Everything `View` can do, plus running the project and maintenance.
    Operate,
    /// Everything `Operate` can do, plus changing the integration itself.
    Administer,
}

impl Permission {
    pub fn as_str(self) -> &'static str {
        match self {
            Permission::View => "view",
            Permission::Operate => "operate",
            Permission::Administer => "administer",
        }
    }
}

/// Something a member can ask the bot to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    Status,
    Resources,
    RecentErrors,
    Logs,
    Start,
    Stop,
    Restart,
    Scan,
    ToggleFeature,
    EditSettings,
    Unlink,
}

impl Action {
    /// The minimum permission that may perform this action.
    ///
    /// Written as an exhaustive `match` with no wildcard arm on purpose: adding
    /// a new action must not silently inherit a permissive default. The
    /// compiler makes the author of the next action choose.
    pub fn required_permission(self) -> Permission {
        match self {
            Action::Status | Action::Resources | Action::RecentErrors | Action::Logs => {
                Permission::View
            }
            Action::Start
            | Action::Stop
            | Action::Restart
            | Action::Scan
            | Action::ToggleFeature => Permission::Operate,
            Action::EditSettings | Action::Unlink => Permission::Administer,
        }
    }

    /// Whether the action changes the project's running state.
    ///
    /// Used to decide what needs a confirmation step and what is worth an
    /// `@mention`, not for authorisation.
    pub fn is_destructive(self) -> bool {
        matches!(self, Action::Stop | Action::Restart | Action::Unlink)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Action::Status => "status",
            Action::Resources => "resources",
            Action::RecentErrors => "recent_errors",
            Action::Logs => "logs",
            Action::Start => "start",
            Action::Stop => "stop",
            Action::Restart => "restart",
            Action::Scan => "scan",
            Action::ToggleFeature => "toggle_feature",
            Action::EditSettings => "edit_settings",
            Action::Unlink => "unlink",
        }
    }

    pub fn parse(text: &str) -> Option<Action> {
        Some(match text {
            "status" => Action::Status,
            "resources" => Action::Resources,
            "recent_errors" => Action::RecentErrors,
            "logs" => Action::Logs,
            "start" => Action::Start,
            "stop" => Action::Stop,
            "restart" => Action::Restart,
            "scan" => Action::Scan,
            "toggle_feature" => Action::ToggleFeature,
            "edit_settings" => Action::EditSettings,
            "unlink" => Action::Unlink,
            _ => return None,
        })
    }

    /// Every action, for building menus and for the storage parity test.
    pub const ALL: &'static [Action] = &[
        Action::Status,
        Action::Resources,
        Action::RecentErrors,
        Action::Logs,
        Action::Start,
        Action::Stop,
        Action::Restart,
        Action::Scan,
        Action::ToggleFeature,
        Action::EditSettings,
        Action::Unlink,
    ];
}

/// Who a grant applies to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Subject {
    Role { id: Snowflake },
    User { id: Snowflake },
}

/// One "this role or person may do this much".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub subject: Subject,
    pub level: Permission,
}

impl Grant {
    pub fn role(id: Snowflake, level: Permission) -> Self {
        Self {
            subject: Subject::Role { id },
            level,
        }
    }

    pub fn user(id: Snowflake, level: Permission) -> Self {
        Self {
            subject: Subject::User { id },
            level,
        }
    }
}

/// The member asking to do something, as Discord described them in the
/// interaction it just delivered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Actor {
    pub user_id: Snowflake,
    pub roles: BTreeSet<Snowflake>,
    /// Whether Discord says this member owns the server.
    pub is_guild_owner: bool,
}

impl Actor {
    pub fn new(user_id: Snowflake, roles: impl IntoIterator<Item = Snowflake>) -> Self {
        Self {
            user_id,
            roles: roles.into_iter().collect(),
            is_guild_owner: false,
        }
    }

    pub fn as_guild_owner(mut self) -> Self {
        self.is_guild_owner = true;
        self
    }
}

/// Why an action was refused.
///
/// Carries the levels involved so the refusal message can say "this needs
/// operate, you have view" instead of a bare "no", and so the audit entry
/// records the same thing the user was told.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Denied {
    #[error("no permission has been granted to you in this server")]
    NotGranted,
    #[error("you have been blocked from controlling projects from Discord")]
    Blocked,
    #[error("that needs {required} permission; you have {held}")]
    Insufficient {
        required: &'static str,
        held: &'static str,
    },
}

/// The permission configuration for one linked Discord server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessPolicy {
    /// The account that linked this server. Always an administrator, and
    /// deliberately not removable through the grant list — otherwise a mistaken
    /// edit could leave a server that nobody can administer, recoverable only
    /// from the desktop application.
    pub linked_by: Snowflake,
    /// Whether Discord's own server owner is treated as an administrator.
    ///
    /// Defaults to true so that access survives the person who linked the
    /// server leaving it. A cautious operator can turn it off.
    pub allow_guild_owner: bool,
    pub grants: Vec<Grant>,
    /// Members refused regardless of their roles. Checked before anything else
    /// so that adding a block is immediate and does not require unpicking which
    /// role granted the access.
    pub blocked_users: Vec<Snowflake>,
}

impl AccessPolicy {
    pub fn new(linked_by: Snowflake) -> Self {
        Self {
            linked_by,
            allow_guild_owner: true,
            grants: Vec::new(),
            blocked_users: Vec::new(),
        }
    }

    pub fn with_grant(mut self, grant: Grant) -> Self {
        self.grants.push(grant);
        self
    }

    pub fn block(mut self, user: Snowflake) -> Self {
        if !self.blocked_users.contains(&user) {
            self.blocked_users.push(user);
        }
        self
    }

    /// The level this member holds, if any.
    ///
    /// Several grants can match at once — a user grant and two role grants, say.
    /// The highest wins. The alternative, letting a low role grant cap a high
    /// one, would mean giving someone an extra role could silently demote them.
    pub fn permission_for(&self, actor: &Actor) -> Option<Permission> {
        // The linker is checked before the block list: blocking them would make
        // the server unadministrable from Discord, which is the one state this
        // module must never reach.
        if actor.user_id == self.linked_by {
            return Some(Permission::Administer);
        }

        if self.blocked_users.contains(&actor.user_id) {
            return None;
        }

        if self.allow_guild_owner && actor.is_guild_owner {
            return Some(Permission::Administer);
        }

        self.grants
            .iter()
            .filter(|grant| match grant.subject {
                Subject::User { id } => id == actor.user_id,
                Subject::Role { id } => actor.roles.contains(&id),
            })
            .map(|grant| grant.level)
            .max()
    }

    /// Decide whether this member may perform this action, right now.
    ///
    /// Call this from the interaction handler, not from the code that renders
    /// the panel. The buttons are drawn once and clicked later, possibly by
    /// somebody else, possibly after their roles have changed.
    pub fn authorise(&self, actor: &Actor, action: Action) -> Result<Permission, Denied> {
        let required = action.required_permission();

        let held = match self.permission_for(actor) {
            Some(level) => level,
            None => {
                return Err(if self.blocked_users.contains(&actor.user_id) {
                    Denied::Blocked
                } else {
                    Denied::NotGranted
                })
            }
        };

        if held >= required {
            Ok(held)
        } else {
            Err(Denied::Insufficient {
                required: required.as_str(),
                held: held.as_str(),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: u64) -> Snowflake {
        Snowflake::new(value).expect("non-zero")
    }

    fn policy() -> AccessPolicy {
        AccessPolicy::new(id(1))
    }

    #[test]
    fn a_member_with_no_grant_is_refused() {
        let denied = policy()
            .authorise(&Actor::new(id(99), [id(500)]), Action::Status)
            .expect_err("default deny");
        assert_eq!(denied, Denied::NotGranted);
    }

    #[test]
    fn being_in_the_server_is_not_permission_to_stop_a_project() {
        // The failure this test guards against is the classic one: a bot that
        // treats "could send the message" as "was allowed to".
        let policy = policy().with_grant(Grant::role(id(500), Permission::View));
        let actor = Actor::new(id(99), [id(500)]);

        assert!(policy.authorise(&actor, Action::Status).is_ok());
        assert_eq!(
            policy.authorise(&actor, Action::Stop),
            Err(Denied::Insufficient {
                required: "operate",
                held: "view",
            })
        );
    }

    #[test]
    fn a_role_grant_applies_to_everyone_holding_the_role() {
        let policy = policy().with_grant(Grant::role(id(500), Permission::Operate));
        for user in [id(10), id(11), id(12)] {
            assert!(policy
                .authorise(&Actor::new(user, [id(500)]), Action::Restart)
                .is_ok());
        }
    }

    #[test]
    fn the_highest_matching_grant_wins() {
        // Someone with a viewer role and an operator role is an operator.
        // If the lowest won, granting an extra role would demote people.
        let policy = policy()
            .with_grant(Grant::role(id(500), Permission::View))
            .with_grant(Grant::role(id(501), Permission::Operate));

        let actor = Actor::new(id(99), [id(500), id(501)]);
        assert_eq!(policy.permission_for(&actor), Some(Permission::Operate));
    }

    #[test]
    fn a_user_grant_can_exceed_a_role_grant() {
        let policy = policy()
            .with_grant(Grant::role(id(500), Permission::View))
            .with_grant(Grant::user(id(99), Permission::Administer));

        let actor = Actor::new(id(99), [id(500)]);
        assert_eq!(policy.permission_for(&actor), Some(Permission::Administer));
    }

    #[test]
    fn a_blocked_user_is_refused_even_with_a_granting_role() {
        let policy = policy()
            .with_grant(Grant::role(id(500), Permission::Administer))
            .block(id(99));

        let actor = Actor::new(id(99), [id(500)]);
        assert_eq!(policy.permission_for(&actor), None);
        assert_eq!(
            policy.authorise(&actor, Action::Status),
            Err(Denied::Blocked)
        );
    }

    #[test]
    fn a_blocked_guild_owner_is_still_refused() {
        // Ownership of the Discord server is not ownership of the host.
        let policy = policy().block(id(99));
        let actor = Actor::new(id(99), []).as_guild_owner();
        assert_eq!(policy.permission_for(&actor), None);
    }

    #[test]
    fn the_account_that_linked_the_server_cannot_be_locked_out() {
        // Including by blocking them, which is the only way this would
        // otherwise happen — and it would leave nobody able to fix it from
        // Discord.
        let policy = policy().block(id(1));
        let actor = Actor::new(id(1), []);
        assert_eq!(policy.permission_for(&actor), Some(Permission::Administer));
        assert!(policy.authorise(&actor, Action::Unlink).is_ok());
    }

    #[test]
    fn the_guild_owner_is_an_administrator_unless_that_is_turned_off() {
        let mut policy = policy();
        let owner = Actor::new(id(77), []).as_guild_owner();
        assert_eq!(policy.permission_for(&owner), Some(Permission::Administer));

        policy.allow_guild_owner = false;
        assert_eq!(policy.permission_for(&owner), None);
    }

    #[test]
    fn an_operator_cannot_change_the_integration_itself() {
        // Otherwise the first thing a compromised operator does is grant
        // themselves administer.
        let policy = policy().with_grant(Grant::role(id(500), Permission::Operate));
        let actor = Actor::new(id(99), [id(500)]);

        assert!(policy.authorise(&actor, Action::Restart).is_ok());
        assert_eq!(
            policy.authorise(&actor, Action::EditSettings),
            Err(Denied::Insufficient {
                required: "administer",
                held: "operate",
            })
        );
        assert!(policy.authorise(&actor, Action::Unlink).is_err());
    }

    #[test]
    fn read_only_actions_need_only_view() {
        for action in [
            Action::Status,
            Action::Resources,
            Action::RecentErrors,
            Action::Logs,
        ] {
            assert_eq!(
                action.required_permission(),
                Permission::View,
                "{action:?} should be readable by a viewer"
            );
        }
    }

    #[test]
    fn every_action_that_changes_the_project_needs_at_least_operate() {
        for action in Action::ALL {
            if action.is_destructive() {
                assert!(
                    action.required_permission() >= Permission::Operate,
                    "{action:?} changes state and must not be viewable-only"
                );
            }
        }
    }

    #[test]
    fn permissions_are_ordered_from_least_to_most() {
        assert!(Permission::View < Permission::Operate);
        assert!(Permission::Operate < Permission::Administer);
    }

    #[test]
    fn every_action_round_trips_through_its_wire_name() {
        for action in Action::ALL {
            assert_eq!(
                Action::parse(action.as_str()),
                Some(*action),
                "{action:?} must survive the round trip through a custom id"
            );
        }
    }

    #[test]
    fn an_unknown_action_name_is_not_guessed() {
        for bad in ["", "START", "delete_everything", "status "] {
            assert_eq!(Action::parse(bad), None, "{bad:?} should not parse");
        }
    }

    #[test]
    fn a_refusal_says_what_was_needed() {
        // The message goes to the user and the reason goes to the audit log;
        // they must be the same reason.
        let policy = policy().with_grant(Grant::role(id(500), Permission::View));
        let error = policy
            .authorise(&Actor::new(id(99), [id(500)]), Action::Stop)
            .expect_err("denied");
        let message = error.to_string();
        assert!(message.contains("operate"), "got {message}");
        assert!(message.contains("view"), "got {message}");
    }
}
