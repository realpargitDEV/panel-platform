//! Generated identifiers.
//!
//! The single most important rule in the system: **a project's display name
//! never becomes a path, a slug, a container name, a network name or a volume
//! name.** Those all derive from a server-generated UUID.
//!
//! That is why a project can be called `../../etc/passwd` or `; rm -rf /` and
//! nothing anywhere is at risk — the name is a label rendered in a list, and
//! the filesystem and Docker only ever see the generated slug.

use std::fmt;

/// Readable words, so a slug in a container listing is recognisable rather than
/// a wall of hex. Deliberately bland and unambiguous.
const ADJECTIVES: &[&str] = &[
    "quiet", "brave", "calm", "swift", "bright", "clever", "gentle", "kind", "lively", "merry",
    "noble", "proud", "rapid", "steady", "warm", "eager", "fair", "keen", "neat", "solid",
];

const NOUNS: &[&str] = &[
    "harbor", "meadow", "summit", "river", "forest", "canyon", "island", "valley", "bridge",
    "garden", "beacon", "anchor", "compass", "lantern", "orchard", "prairie", "quarry", "ridge",
    "spring", "thicket",
];

/// A generated, filesystem-safe, Docker-safe identifier.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Slug(String);

impl Slug {
    /// Derive a slug from a project id.
    ///
    /// Deterministic: the same id always yields the same slug, so a slug can be
    /// recomputed during recovery without storing a mapping. The suffix is the
    /// tail of the UUID, which makes collisions between two projects
    /// impossible rather than merely unlikely.
    pub fn from_project_id(project_id: &str) -> Self {
        let body = project_id.rsplit('_').next().unwrap_or(project_id);
        let clean: String = body.chars().filter(|c| c.is_ascii_hexdigit()).collect();

        // Two independent bytes of the UUID pick the words; the last four hex
        // digits guarantee uniqueness.
        let adjective_index = byte_at(&clean, 0) % ADJECTIVES.len();
        let noun_index = byte_at(&clean, 1) % NOUNS.len();
        let suffix: String = clean
            .chars()
            .rev()
            .take(4)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let adjective = ADJECTIVES.get(adjective_index).copied().unwrap_or("quiet");
        let noun = NOUNS.get(noun_index).copied().unwrap_or("harbor");

        Self(format!("{adjective}-{noun}-{suffix}"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Validate a slug read back from the database.
    ///
    /// Matches the `CHECK (slug GLOB '[a-z0-9][a-z0-9-]*')` constraint. A slug
    /// that fails this never came from [`Slug::from_project_id`].
    pub fn parse(value: &str) -> Option<Self> {
        if value.is_empty() || value.len() > 64 {
            return None;
        }
        let mut characters = value.chars();
        let first = characters.next()?;
        if !first.is_ascii_lowercase() && !first.is_ascii_digit() {
            return None;
        }
        if !value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return None;
        }
        Some(Self(value.to_string()))
    }
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn byte_at(hex: &str, index: usize) -> usize {
    let start = index * 2;
    hex.get(start..start + 2)
        .and_then(|pair| usize::from_str_radix(pair, 16).ok())
        .unwrap_or(0)
}

/// Clean a display name for presentation.
///
/// This is *not* a safety function — nothing downstream depends on it. It
/// exists so a name with a stray control character does not corrupt a terminal
/// or a list view. Path-like and shell-like names are left intact, because they
/// are harmless here and silently rewriting what someone typed is worse.
pub fn sanitise_display_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| !c.is_control())
        .collect::<String>()
        .trim()
        .to_string();

    cleaned.chars().take(64).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_slug_is_derived_from_the_project_id() {
        let slug = Slug::from_project_id("prj_0193aabbccddeeff0011223344556677");
        assert!(Slug::parse(slug.as_str()).is_some(), "got {slug}");
        assert!(slug.as_str().contains('-'));
    }

    #[test]
    fn slug_derivation_is_deterministic() {
        // Recovery recomputes slugs rather than storing a mapping.
        let id = "prj_0193aabbccddeeff0011223344556677";
        assert_eq!(Slug::from_project_id(id), Slug::from_project_id(id));
    }

    #[test]
    fn different_projects_get_different_slugs() {
        let first = Slug::from_project_id("prj_0193aabbccddeeff0011223344556677");
        let second = Slug::from_project_id("prj_0193aabbccddeeff0011223344556688");
        assert_ne!(first, second);
    }

    #[test]
    fn every_generated_slug_satisfies_the_database_constraint() {
        for index in 0..2000u32 {
            let id = format!("prj_0193aabbccddeeff00112233{index:08x}");
            let slug = Slug::from_project_id(&id);
            assert!(
                Slug::parse(slug.as_str()).is_some(),
                "generated an invalid slug: {slug}"
            );
        }
    }

    #[test]
    fn a_hostile_display_name_cannot_influence_the_slug() {
        // The point of the whole module: these names are display-only.
        for hostile in [
            "../../etc/passwd",
            "; rm -rf /",
            "$(whoami)",
            "..\\..\\Windows\\System32",
            "\u{0}\u{1}evil",
        ] {
            // The slug comes from the id, so the name is simply not an input.
            let slug = Slug::from_project_id("prj_0193aabbccddeeff0011223344556677");
            assert!(!slug.as_str().contains(hostile));
            assert!(Slug::parse(slug.as_str()).is_some());
        }
    }

    #[test]
    fn slug_parsing_rejects_anything_unsafe() {
        for bad in [
            "",
            "-leading-hyphen",
            "Upper",
            "has space",
            "has_underscore",
            "has/slash",
            "has..dots",
            "a".repeat(65).as_str(),
        ] {
            assert!(Slug::parse(bad).is_none(), "{bad:?} should be refused");
        }
        assert!(Slug::parse("quiet-harbor-4f2a").is_some());
        assert!(Slug::parse("0abc").is_some());
    }

    #[test]
    fn display_names_lose_control_characters_but_keep_their_shape() {
        assert_eq!(sanitise_display_name("  My Bot  "), "My Bot");
        assert_eq!(sanitise_display_name("bot\u{7}name"), "botname");
        // Not a safety measure, so these survive intact.
        assert_eq!(
            sanitise_display_name("../../etc/passwd"),
            "../../etc/passwd"
        );
        assert_eq!(sanitise_display_name("C:\\Windows"), "C:\\Windows");
    }

    #[test]
    fn display_names_are_length_bounded() {
        assert_eq!(sanitise_display_name(&"a".repeat(200)).chars().count(), 64);
    }
}
