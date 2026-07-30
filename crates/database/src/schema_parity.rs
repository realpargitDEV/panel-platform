//! Keeps the Rust enums and the SQL `CHECK` constraints in agreement.
//!
//! Both exist for good reasons — the constraint protects the file from any
//! writer, the Rust enum protects the code from typos — but two lists of the
//! same values drift. These helpers extract the constraint lists from the
//! migration text so a test can compare them, turning a drift into a failed
//! build instead of a runtime insert that a `CHECK` refuses in production.

/// The `CREATE TABLE` body for one table, or `None` if there is no such table.
///
/// Accepts a quoted table name as well as a bare one. Migration text is written
/// bare, but the definition SQLite keeps in `sqlite_master` for a table that has
/// been through an `ALTER TABLE ... RENAME TO` is quoted — and reading the live
/// schema is the only way to be sure which migration currently owns a table.
pub fn table_body<'a>(sql: &'a str, table: &str) -> Option<&'a str> {
    let start = [
        format!("CREATE TABLE {table} ("),
        format!("CREATE TABLE \"{table}\" ("),
    ]
    .iter()
    .find_map(|needle| sql.find(needle.as_str()).map(|at| at + needle.len()))?;

    let rest = sql.get(start..)?;

    // Walk to the matching close paren, tracking nesting so the parentheses
    // inside CHECK expressions do not end the body early.
    let mut depth = 1usize;
    for (index, character) in rest.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return rest.get(..index);
                }
            }
            _ => {}
        }
    }
    None
}

/// The quoted values of `<column> IN (...)` within a table body.
pub fn check_values(table_body: &str, column: &str) -> Option<Vec<String>> {
    let needle = format!("{column} IN (");
    let start = table_body.find(&needle)? + needle.len();
    let rest = table_body.get(start..)?;
    let end = rest.find(')')?;
    let list = rest.get(..end)?;

    let values: Vec<String> = list
        .split(',')
        .map(str::trim)
        .filter_map(|token| token.strip_prefix('\'')?.strip_suffix('\''))
        .map(str::to_string)
        .collect();

    (!values.is_empty()).then_some(values)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
CREATE TABLE things (
    id TEXT PRIMARY KEY,
    status TEXT NOT NULL CHECK (status IN ('A','B','C')),
    CHECK (n BETWEEN 1 AND 2)
);
CREATE TABLE others (
    status TEXT NOT NULL CHECK (status IN ('X'))
);";

    #[test]
    fn extracts_the_right_table_body() {
        let body = table_body(SAMPLE, "things").expect("things");
        assert!(body.contains("'A','B','C'"));
        assert!(!body.contains("'X'"), "body leaked into the next table");
    }

    #[test]
    fn scopes_a_column_to_its_own_table() {
        // `status` exists in both tables; each must yield its own list.
        let things = table_body(SAMPLE, "things").expect("things");
        let others = table_body(SAMPLE, "others").expect("others");
        assert_eq!(
            check_values(things, "status").expect("things.status"),
            ["A", "B", "C"]
        );
        assert_eq!(
            check_values(others, "status").expect("others.status"),
            ["X"]
        );
    }

    #[test]
    fn nested_parentheses_do_not_end_the_body_early() {
        let body = table_body(SAMPLE, "things").expect("things");
        assert!(
            body.contains("BETWEEN 1 AND 2"),
            "stopped at a nested paren"
        );
    }

    #[test]
    fn a_renamed_tables_quoted_definition_is_still_found() {
        // What `sqlite_master` holds after a table rebuild.
        let renamed = "CREATE TABLE \"things\" (\n    status TEXT CHECK (status IN ('A','B'))\n)";
        let body = table_body(renamed, "things").expect("quoted things");
        assert_eq!(check_values(body, "status").expect("status"), ["A", "B"]);
    }

    #[test]
    fn missing_things_return_none() {
        assert!(table_body(SAMPLE, "nope").is_none());
        let body = table_body(SAMPLE, "things").expect("things");
        assert!(check_values(body, "absent_column").is_none());
    }
}
