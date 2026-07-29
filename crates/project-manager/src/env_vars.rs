//! The environment-variable manager.
//!
//! Variables reach a project by being handed to Docker as a structured list,
//! never by being pasted into a shell command, so this module is not defending
//! against shell metacharacters — [`crate::names`] and the container spec cover
//! that. What it defends against is subtler: a name that is not a legal
//! environment-variable name at all, a name the product reserves for itself, a
//! value carrying control characters that corrupt whatever reads it, and the
//! quiet duplicate that means one of the user's two values silently wins.
//!
//! Secret values leave here exactly once — on the way to encryption. Every read
//! path returns [`EnvVarView`], which cannot carry a secret value even by
//! mistake, because the field does not exist.

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EnvVarError {
    #[error("a variable name cannot be empty")]
    EmptyName,
    #[error("`{0}` is not a valid variable name: use letters, digits and underscores, not starting with a digit")]
    InvalidName(String),
    #[error("`{0}` is reserved by Panel Platform and cannot be set on a project")]
    ReservedName(String),
    #[error("a variable name cannot be longer than {limit} characters")]
    NameTooLong { limit: usize },
    #[error("the value of `{name}` is longer than the {limit} character limit")]
    ValueTooLong { name: String, limit: usize },
    #[error("the value of `{0}` contains a control character")]
    ControlCharacter(String),
    #[error("`{0}` is defined more than once")]
    Duplicate(String),
    #[error("a project cannot have more than {limit} environment variables")]
    TooMany { limit: usize },
}

pub const MAX_NAME_LENGTH: usize = 128;
pub const MAX_VALUE_LENGTH: usize = 32 * 1024;
pub const MAX_VARIABLES: usize = 500;

/// Names the product injects into every container itself.
///
/// A project that could set these would be describing its own identity to code
/// that trusts it. The prefix is reserved wholesale rather than by exact name so
/// adding an injected variable later cannot silently collide with a user's.
const RESERVED_PREFIX: &str = "PROJECT_HOST_";

/// Names that change how the dynamic loader behaves before a program's own code
/// runs. Inside a container they are the user's own foot to shoot, but they are
/// also the standard way to turn "run my script" into "run my code inside
/// something else", and no legitimate project template needs them.
const REFUSED_NAMES: &[&str] = &[
    "LD_PRELOAD",
    "LD_AUDIT",
    "LD_LIBRARY_PATH",
    "DYLD_INSERT_LIBRARIES",
    "DYLD_LIBRARY_PATH",
];

/// One variable as the user defined it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVar {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
}

impl EnvVar {
    pub fn plain(key: &str, value: &str) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            is_secret: false,
        }
    }

    pub fn secret(key: &str, value: &str) -> Self {
        Self {
            key: key.to_string(),
            value: value.to_string(),
            is_secret: true,
        }
    }
}

/// What the client is allowed to see.
///
/// A secret's value is absent, not blanked: there is no field to accidentally
/// populate, so no future change to a handler can start leaking one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EnvVarView {
    pub key: String,
    pub is_secret: bool,
    /// Present only for non-secret variables.
    pub value: Option<String>,
    /// For a secret: enough to recognise a value without revealing it.
    pub masked: Option<String>,
    /// Whether changing this variable needs the project restarted to take
    /// effect. Always true today — a container's environment is fixed at
    /// creation — and modelled explicitly so the UI states it rather than
    /// leaving the user to discover it.
    pub restart_required: bool,
}

impl From<&EnvVar> for EnvVarView {
    fn from(var: &EnvVar) -> Self {
        Self {
            key: var.key.clone(),
            is_secret: var.is_secret,
            value: if var.is_secret {
                None
            } else {
                Some(var.value.clone())
            },
            masked: if var.is_secret {
                Some(mask(&var.value))
            } else {
                None
            },
            restart_required: true,
        }
    }
}

/// A recognisable stand-in for a secret.
///
/// The length is deliberately not proportional to the real one — a mask that
/// grows with the secret tells an observer how long it is.
pub fn mask(value: &str) -> String {
    if value.is_empty() {
        return String::new();
    }
    let visible: String = value.chars().take(2).collect();
    if value.chars().count() <= 4 {
        return "••••••••".to_string();
    }
    format!("{visible}••••••••")
}

/// Validate a variable name.
pub fn validate_key(key: &str) -> Result<(), EnvVarError> {
    if key.is_empty() {
        return Err(EnvVarError::EmptyName);
    }
    if key.len() > MAX_NAME_LENGTH {
        return Err(EnvVarError::NameTooLong {
            limit: MAX_NAME_LENGTH,
        });
    }

    let mut chars = key.chars();
    let first = chars.next().unwrap_or('0');
    if !(first.is_ascii_alphabetic() || first == '_') {
        return Err(EnvVarError::InvalidName(key.to_string()));
    }
    if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(EnvVarError::InvalidName(key.to_string()));
    }

    let upper = key.to_ascii_uppercase();
    if upper.starts_with(RESERVED_PREFIX) || REFUSED_NAMES.contains(&upper.as_str()) {
        return Err(EnvVarError::ReservedName(key.to_string()));
    }

    Ok(())
}

/// Validate a value.
///
/// Newlines are allowed — certificates and private keys are routine — but every
/// other control character is refused. A NUL truncates the value in the C API
/// underneath Docker, and an escape sequence in a value ends up rendered by
/// whatever reads the logs.
pub fn validate_value(key: &str, value: &str) -> Result<(), EnvVarError> {
    if value.len() > MAX_VALUE_LENGTH {
        return Err(EnvVarError::ValueTooLong {
            name: key.to_string(),
            limit: MAX_VALUE_LENGTH,
        });
    }
    if value
        .chars()
        .any(|c| c.is_control() && c != '\n' && c != '\r' && c != '\t')
    {
        return Err(EnvVarError::ControlCharacter(key.to_string()));
    }
    Ok(())
}

/// Validate a whole set, including duplicates and the count.
///
/// Duplicate detection is case-sensitive because environment variables are, on
/// both platforms Docker runs containers on. `Path` and `PATH` really are two
/// variables inside a Linux container, and pretending otherwise would refuse a
/// legal configuration.
pub fn validate_set(vars: &[EnvVar]) -> Result<(), EnvVarError> {
    if vars.len() > MAX_VARIABLES {
        return Err(EnvVarError::TooMany {
            limit: MAX_VARIABLES,
        });
    }

    let mut seen = std::collections::HashSet::new();
    for var in vars {
        validate_key(&var.key)?;
        validate_value(&var.key, &var.value)?;
        if !seen.insert(var.key.as_str()) {
            return Err(EnvVarError::Duplicate(var.key.clone()));
        }
    }
    Ok(())
}

/// A parsed `.env` file.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedDotenv {
    pub vars: Vec<EnvVar>,
    /// Lines that could not be understood, with their line numbers. Reported
    /// rather than dropped: a silently ignored line is how a missing token in
    /// production gets diagnosed three hours later.
    pub warnings: Vec<DotenvWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DotenvWarning {
    pub line: usize,
    pub reason: String,
}

/// Names that look like secrets, used to preselect the secret toggle on import.
///
/// A heuristic, and treated as one: it only ever *adds* protection, and the user
/// can clear the toggle. Getting it wrong in the safe direction costs a click.
fn looks_secret(key: &str) -> bool {
    const MARKERS: &[&str] = &[
        "SECRET",
        "TOKEN",
        "PASSWORD",
        "PASSWD",
        "APIKEY",
        "API_KEY",
        "PRIVATE",
        "CREDENTIAL",
        "AUTH",
        "SIGNING",
        "SESSION",
        "DSN",
        "WEBHOOK",
    ];
    let upper = key.to_ascii_uppercase();
    MARKERS.iter().any(|marker| upper.contains(marker))
        // `KEY` alone matches too much (`KEYBOARD_LAYOUT`), so require it to be
        // a whole word.
        || upper.split('_').any(|part| part == "KEY")
}

/// Parse a `.env` file.
///
/// Supports the syntax people actually have in their files: optional `export`,
/// `#` comments, single- and double-quoted values, escapes inside double
/// quotes, and inline comments after unquoted values. Deliberately does *not*
/// support variable interpolation (`${OTHER}`) — resolving one variable from
/// another silently changes what a value means depending on order, and the
/// literal text is what the user can see in the editor.
pub fn parse_dotenv(text: &str) -> ParsedDotenv {
    let mut parsed = ParsedDotenv::default();
    let mut seen: BTreeMap<String, usize> = BTreeMap::new();

    for (index, raw) in text.lines().enumerate() {
        let line_number = index + 1;
        let line = raw.trim_start_matches('\u{feff}').trim();

        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        let without_export = line.strip_prefix("export ").unwrap_or(line).trim_start();

        let Some((key_part, value_part)) = without_export.split_once('=') else {
            parsed.warnings.push(DotenvWarning {
                line: line_number,
                reason: "no `=` on the line".to_string(),
            });
            continue;
        };

        let key = key_part.trim().to_string();
        if let Err(error) = validate_key(&key) {
            parsed.warnings.push(DotenvWarning {
                line: line_number,
                reason: error.to_string(),
            });
            continue;
        }

        let value = match parse_value(value_part.trim()) {
            Ok(value) => value,
            Err(reason) => {
                parsed.warnings.push(DotenvWarning {
                    line: line_number,
                    reason,
                });
                continue;
            }
        };

        if let Err(error) = validate_value(&key, &value) {
            parsed.warnings.push(DotenvWarning {
                line: line_number,
                reason: error.to_string(),
            });
            continue;
        }

        // A repeated key in a file is the user's own mistake, not an attack.
        // The last definition wins — matching what a shell does when it sources
        // the file — and the shadowed line is reported.
        if let Some(previous) = seen.insert(key.clone(), line_number) {
            parsed.warnings.push(DotenvWarning {
                line: previous,
                reason: format!(
                    "`{key}` is redefined on line {line_number} and this value is ignored"
                ),
            });
            parsed.vars.retain(|var| var.key != key);
        }

        parsed.vars.push(EnvVar {
            is_secret: looks_secret(&key),
            key,
            value,
        });
    }

    parsed
}

/// Unquote and unescape one value.
fn parse_value(raw: &str) -> Result<String, String> {
    if raw.is_empty() {
        return Ok(String::new());
    }

    let bytes = raw.as_bytes();
    let first = bytes.first().copied();

    if first == Some(b'\'') {
        // Single quotes are literal, so the only question is where they end.
        return match raw.get(1..).and_then(|rest| rest.rfind('\'')) {
            Some(end) if end + 2 <= raw.len() => {
                Ok(raw.get(1..=end).unwrap_or_default().to_string())
            }
            _ => Err("the single-quoted value is not closed".to_string()),
        };
    }

    if first == Some(b'"') {
        let Some(rest) = raw.get(1..) else {
            return Err("the quoted value is not closed".to_string());
        };
        let mut out = String::new();
        let mut chars = rest.chars();
        loop {
            match chars.next() {
                None => return Err("the quoted value is not closed".to_string()),
                Some('"') => return Ok(out),
                Some('\\') => match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('r') => out.push('\r'),
                    Some('t') => out.push('\t'),
                    Some('\\') => out.push('\\'),
                    Some('"') => out.push('"'),
                    Some(other) => {
                        // An unknown escape keeps both characters rather than
                        // being reinterpreted — a Windows path in a value
                        // should survive intact.
                        out.push('\\');
                        out.push(other);
                    }
                    None => return Err("the quoted value ends with a backslash".to_string()),
                },
                Some(other) => out.push(other),
            }
        }
    }

    // Unquoted: an unescaped `#` starts a comment, and trailing whitespace is
    // not part of the value.
    let end = raw.find(" #").unwrap_or(raw.len());
    Ok(raw.get(..end).unwrap_or(raw).trim_end().to_string())
}

/// Render a `.env.example`.
///
/// Secret values are replaced, never masked: a mask still shows a prefix, and
/// this file is the one most likely to be committed to a repository.
pub fn export_example(vars: &[EnvVar]) -> String {
    let mut out = String::from(
        "# Generated by Panel Platform.\n\
         # Secret values are omitted. Fill them in and rename this file to `.env`.\n\n",
    );
    let mut sorted: Vec<&EnvVar> = vars.iter().collect();
    sorted.sort_by(|a, b| a.key.cmp(&b.key));

    for var in sorted {
        if var.is_secret {
            out.push_str(&format!("{}=\n", var.key));
        } else {
            out.push_str(&format!("{}={}\n", var.key, quote_if_needed(&var.value)));
        }
    }
    out
}

/// Quote a value for a `.env` file when leaving it bare would change it.
fn quote_if_needed(value: &str) -> String {
    let needs_quotes = value.is_empty()
        || value.contains(char::is_whitespace)
        || value.contains('#')
        || value.contains('"')
        || value.starts_with('\'');
    if !needs_quotes {
        return value.to_string();
    }
    let escaped = value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
        .replace('\r', "\\r");
    format!("\"{escaped}\"")
}

/// The Docker form: `KEY=value`, one per element.
///
/// Returned as a `Vec<String>` handed to the container-create call as a
/// structured argument, never joined into a command line.
pub fn to_docker_env(vars: &[EnvVar]) -> Vec<String> {
    vars.iter()
        .map(|var| format!("{}={}", var.key, var.value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_names_are_accepted() {
        for key in ["PORT", "NODE_ENV", "_INTERNAL", "DB2_URL", "a"] {
            assert!(validate_key(key).is_ok(), "{key} should be valid");
        }
    }

    #[test]
    fn malformed_names_are_refused() {
        for key in [
            "2FAST",
            "has-dash",
            "has space",
            "has.dot",
            "has$sign",
            "ünïcode",
        ] {
            assert!(
                matches!(validate_key(key), Err(EnvVarError::InvalidName(_))),
                "{key} should be refused"
            );
        }
        assert!(matches!(validate_key(""), Err(EnvVarError::EmptyName)));
    }

    #[test]
    fn the_products_own_prefix_is_reserved() {
        for key in ["PROJECT_HOST_ID", "project_host_agent_url"] {
            assert!(
                matches!(validate_key(key), Err(EnvVarError::ReservedName(_))),
                "{key} should be reserved"
            );
        }
    }

    #[test]
    fn loader_variables_are_refused() {
        for key in [
            "LD_PRELOAD",
            "ld_preload",
            "LD_LIBRARY_PATH",
            "DYLD_INSERT_LIBRARIES",
        ] {
            assert!(
                matches!(validate_key(key), Err(EnvVarError::ReservedName(_))),
                "{key} should be refused"
            );
        }
    }

    #[test]
    fn a_nul_or_escape_in_a_value_is_refused_but_a_newline_is_not() {
        assert!(validate_value("K", "line one\nline two").is_ok());
        assert!(validate_value("K", "tab\there").is_ok());
        assert!(matches!(
            validate_value("K", "before\0after"),
            Err(EnvVarError::ControlCharacter(_))
        ));
        assert!(matches!(
            validate_value("K", "\u{1b}[31mred"),
            Err(EnvVarError::ControlCharacter(_))
        ));
    }

    #[test]
    fn a_duplicate_in_a_set_is_refused() {
        let vars = vec![EnvVar::plain("PORT", "3000"), EnvVar::plain("PORT", "4000")];
        assert!(matches!(
            validate_set(&vars),
            Err(EnvVarError::Duplicate(key)) if key == "PORT"
        ));
    }

    #[test]
    fn case_differing_names_are_two_variables_not_a_duplicate() {
        let vars = vec![EnvVar::plain("Path", "/a"), EnvVar::plain("PATH", "/b")];
        assert!(validate_set(&vars).is_ok());
    }

    #[test]
    fn a_set_is_capped() {
        let vars: Vec<EnvVar> = (0..MAX_VARIABLES + 1)
            .map(|i| EnvVar::plain(&format!("VAR_{i}"), "x"))
            .collect();
        assert!(matches!(
            validate_set(&vars),
            Err(EnvVarError::TooMany { .. })
        ));
    }

    #[test]
    fn a_secret_never_reaches_the_view() {
        let view = EnvVarView::from(&EnvVar::secret("DISCORD_TOKEN", "super-secret-value"));
        assert_eq!(view.value, None);
        assert_eq!(view.masked.as_deref(), Some("su••••••••"));
        assert!(view.restart_required);
    }

    #[test]
    fn a_non_secret_keeps_its_value() {
        let view = EnvVarView::from(&EnvVar::plain("PORT", "3000"));
        assert_eq!(view.value.as_deref(), Some("3000"));
        assert_eq!(view.masked, None);
    }

    #[test]
    fn a_short_secret_reveals_nothing_at_all() {
        assert_eq!(mask("abc"), "••••••••");
        assert_eq!(mask(""), "");
    }

    #[test]
    fn a_mask_does_not_disclose_the_length() {
        assert_eq!(mask("abcdefghij").len(), mask("abcdefghijklmnop").len());
    }

    #[test]
    fn a_plain_dotenv_file_parses() {
        let parsed = parse_dotenv("PORT=3000\nNODE_ENV=production\n");
        assert_eq!(
            parsed.vars,
            vec![
                EnvVar::plain("PORT", "3000"),
                EnvVar::plain("NODE_ENV", "production"),
            ]
        );
        assert!(parsed.warnings.is_empty());
    }

    #[test]
    fn comments_blank_lines_and_export_are_handled() {
        let parsed =
            parse_dotenv("# a comment\n\n  export PORT=3000\n\t# indented comment\nNAME=app\n");
        assert_eq!(parsed.vars.len(), 2);
        assert_eq!(parsed.vars[0].key, "PORT");
        assert_eq!(parsed.vars[1].value, "app");
    }

    #[test]
    fn quoted_values_keep_their_spaces_and_hashes() {
        let parsed = parse_dotenv("A=\"hello world\"\nB='it #4'\nC=bare # trailing\n");
        assert_eq!(parsed.vars[0].value, "hello world");
        assert_eq!(parsed.vars[1].value, "it #4");
        assert_eq!(parsed.vars[2].value, "bare");
    }

    #[test]
    fn escapes_inside_double_quotes_are_interpreted() {
        let parsed = parse_dotenv("KEYFILE=\"line1\\nline2\"\nQ=\"say \\\"hi\\\"\"\n");
        assert_eq!(parsed.vars[0].value, "line1\nline2");
        assert_eq!(parsed.vars[1].value, "say \"hi\"");
    }

    #[test]
    fn an_unknown_escape_is_left_alone_so_windows_paths_survive() {
        let parsed = parse_dotenv("P=\"C:\\Users\\app\"\n");
        assert_eq!(parsed.vars[0].value, "C:\\Users\\app");
    }

    #[test]
    fn an_unclosed_quote_is_a_warning_not_a_silent_value() {
        let parsed = parse_dotenv("A=\"never closed\nB=fine\n");
        assert_eq!(parsed.vars.len(), 1);
        assert_eq!(parsed.vars[0].key, "B");
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 1);
    }

    #[test]
    fn a_line_without_an_equals_is_reported() {
        let parsed = parse_dotenv("PORT=3000\nthis is not a variable\n");
        assert_eq!(parsed.vars.len(), 1);
        assert_eq!(parsed.warnings.len(), 1);
        assert!(parsed.warnings[0].reason.contains('='));
    }

    #[test]
    fn an_invalid_name_is_reported_rather_than_imported() {
        let parsed = parse_dotenv("not-a-name=x\nGOOD=y\n");
        assert_eq!(parsed.vars.len(), 1);
        assert_eq!(parsed.vars[0].key, "GOOD");
        assert_eq!(parsed.warnings.len(), 1);
    }

    #[test]
    fn a_redefined_key_keeps_the_last_value_and_warns() {
        let parsed = parse_dotenv("PORT=3000\nPORT=4000\n");
        assert_eq!(parsed.vars.len(), 1);
        assert_eq!(parsed.vars[0].value, "4000");
        assert_eq!(parsed.warnings.len(), 1);
        assert_eq!(parsed.warnings[0].line, 1);
    }

    #[test]
    fn secret_looking_names_are_preselected_as_secret() {
        let parsed = parse_dotenv(
            "DISCORD_TOKEN=abc\nAPI_KEY=def\nDATABASE_PASSWORD=ghi\nSTRIPE_SECRET=jkl\nPORT=3000\nKEYBOARD_LAYOUT=us\n",
        );
        let secret: Vec<&str> = parsed
            .vars
            .iter()
            .filter(|v| v.is_secret)
            .map(|v| v.key.as_str())
            .collect();
        assert_eq!(
            secret,
            vec![
                "DISCORD_TOKEN",
                "API_KEY",
                "DATABASE_PASSWORD",
                "STRIPE_SECRET"
            ]
        );
    }

    #[test]
    fn an_empty_value_is_legal() {
        let parsed = parse_dotenv("EMPTY=\nALSO=\"\"\n");
        assert_eq!(parsed.vars.len(), 2);
        assert_eq!(parsed.vars[0].value, "");
        assert_eq!(parsed.vars[1].value, "");
    }

    #[test]
    fn a_byte_order_mark_does_not_break_the_first_line() {
        let parsed = parse_dotenv("\u{feff}PORT=3000\n");
        assert_eq!(parsed.vars.len(), 1);
        assert_eq!(parsed.vars[0].key, "PORT");
    }

    #[test]
    fn the_example_export_omits_secret_values_entirely() {
        let vars = vec![
            EnvVar::secret("DISCORD_TOKEN", "super-secret"),
            EnvVar::plain("PORT", "3000"),
        ];
        let example = export_example(&vars);
        assert!(example.contains("DISCORD_TOKEN=\n"));
        assert!(example.contains("PORT=3000\n"));
        assert!(
            !example.contains("super-secret") && !example.contains("su••"),
            "no part of a secret may appear: {example}"
        );
    }

    #[test]
    fn the_example_export_quotes_values_that_need_it() {
        let vars = vec![
            EnvVar::plain("MESSAGE", "hello world"),
            EnvVar::plain("EMPTY", ""),
            EnvVar::plain("HASH", "a#b"),
        ];
        let example = export_example(&vars);
        assert!(example.contains("MESSAGE=\"hello world\""));
        assert!(example.contains("EMPTY=\"\""));
        assert!(example.contains("HASH=\"a#b\""));
    }

    #[test]
    fn an_exported_example_reparses_to_the_same_values() {
        // The round trip is the real test of the quoting rules.
        let vars = vec![
            EnvVar::plain("MESSAGE", "hello world"),
            EnvVar::plain("HASH", "a#b"),
            EnvVar::plain("QUOTED", "say \"hi\""),
            EnvVar::plain("MULTI", "one\ntwo"),
        ];
        let reparsed = parse_dotenv(&export_example(&vars));
        for original in &vars {
            let found = reparsed
                .vars
                .iter()
                .find(|v| v.key == original.key)
                .unwrap_or_else(|| panic!("{} missing", original.key));
            assert_eq!(
                found.value, original.value,
                "{} round-tripped wrong",
                original.key
            );
        }
    }

    #[test]
    fn the_docker_form_is_a_list_not_a_command_line() {
        let vars = vec![
            EnvVar::plain("PORT", "3000"),
            // A value that would be catastrophic if this were ever joined into
            // a shell string.
            EnvVar::plain("EVIL", "x; rm -rf / #"),
        ];
        assert_eq!(
            to_docker_env(&vars),
            vec!["PORT=3000".to_string(), "EVIL=x; rm -rf / #".to_string()]
        );
    }
}
