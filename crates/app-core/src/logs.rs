//! Reading one project's console.
//!
//! Every project has its own buffer, its own sequence and its own file. Nothing
//! here takes a set of projects, and that is the design rather than an
//! omission: five projects running at once must produce five consoles, and the
//! only way to be sure they never interleave is for there to be no code path
//! that could interleave them.
//!
//! # Live and historical are different questions
//!
//! A running project's console comes from its supervisor's in-memory buffer,
//! which is cheap, ordered, and carries a cursor so a window can poll for what
//! it has not seen. A stopped project has no supervisor, so its console comes
//! from the file on disk — which is the durable record and the only thing left
//! once the process is gone.
//!
//! The two are presented as one type, because a user switching between a
//! running project and a stopped one is not asking a different question.
//! [`Console::live`] says which they got, so the window can offer "follow"
//! only where following means something.

use std::path::PathBuf;

use project_host_host_runner::{LogLine, Stream};

use crate::state::AppState;

/// How many lines are read back from a file.
///
/// The buffer keeps two thousand for a running project; a stopped one gets the
/// same, so switching between the two does not silently change how much
/// history there is.
const FILE_LINES: usize = 2_000;

/// A project's output, and where to carry on reading from.
#[derive(Debug, Clone, Default)]
pub struct Console {
    pub lines: Vec<LogLine>,
    /// The sequence to pass as `since` on the next poll.
    ///
    /// Zero for a file-backed console: a file has no sequence, so polling one
    /// re-reads it rather than pretending to have a cursor into it.
    pub cursor: u64,
    /// Whether this came from a running supervisor. `false` means the project
    /// is not running and what is shown is the record of when it was.
    pub live: bool,
}

/// Read a project's console.
///
/// `since` is the cursor from the previous call, or zero to start. It is
/// ignored for a project that is not running, because a file cannot answer it.
pub async fn read(app: &AppState, project_id: &str, since: u64) -> Console {
    if let Some(handle) = app.host_projects().handle(project_id).await {
        let (lines, cursor) = handle.logs_since(since);
        return Console {
            lines,
            cursor,
            live: true,
        };
    }

    from_file(app, project_id).await
}

/// The last lines of the project's log file.
///
/// Best effort in both directions: a project that has never run has no file,
/// and an empty console is the right answer for it rather than an error.
async fn from_file(app: &AppState, project_id: &str) -> Console {
    let Ok(Some(project)) =
        project_host_database::projects::find_project(app.database(), project_id).await
    else {
        return Console::default();
    };

    let path = latest_log_file(&app.logs_root(), &project.slug);
    let Some(path) = path else {
        return Console::default();
    };

    // On a blocking thread: reading a few hundred kilobytes off a slow disk on
    // the async runtime would stall every other command for as long as it took.
    let read = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
        .await
        .ok()
        .and_then(Result::ok)
        .unwrap_or_default();

    let mut lines: Vec<&str> = read.lines().collect();
    if lines.len() > FILE_LINES {
        lines.drain(..lines.len() - FILE_LINES);
    }

    Console {
        lines: lines
            .into_iter()
            .enumerate()
            .map(|(index, line)| parse_line(index as u64 + 1, line))
            .collect(),
        cursor: 0,
        live: false,
    }
}

/// The most recent day's log file for a project, if there is one.
///
/// Files are named `run-YYYY-MM-DD.log`, so the newest is the last in
/// lexicographic order — which is the whole reason the date is written that way
/// round.
fn latest_log_file(logs_root: &std::path::Path, slug: &str) -> Option<PathBuf> {
    let directory = logs_root.join("projects").join(slug);
    let mut newest: Option<PathBuf> = None;

    for entry in std::fs::read_dir(&directory).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("log") {
            continue;
        }
        if newest.as_ref().is_none_or(|current| path > *current) {
            newest = Some(path);
        }
    }

    newest
}

/// Turn a written log line back into a structured one.
///
/// The file format is `<timestamp> [<stream>] <text>`, written by the pump. A
/// line that does not match — a file written by an older build, or a project
/// that printed a bare newline — is returned as its own text with no
/// timestamp, which is better than dropping it.
fn parse_line(seq: u64, raw: &str) -> LogLine {
    let fallback = || LogLine {
        seq,
        at: String::new(),
        stream: Stream::Stdout,
        text: raw.to_string(),
    };

    // `2026-08-15T09:41:02Z [stderr] text`
    let Some((timestamp, rest)) = raw.split_once(' ') else {
        return fallback();
    };
    if timestamp.len() != 20 || !timestamp.ends_with('Z') {
        return fallback();
    }
    let Some(rest) = rest.strip_prefix('[') else {
        return fallback();
    };
    let Some((stream, text)) = rest.split_once("] ") else {
        return fallback();
    };

    LogLine {
        seq,
        at: timestamp.to_string(),
        stream: match stream {
            "stderr" => Stream::Stderr,
            "system" => Stream::System,
            _ => Stream::Stdout,
        },
        text: text.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_written_line_parses_back_into_its_parts() {
        let line = parse_line(7, "2026-08-15T09:41:02Z [stderr] Error: EADDRINUSE");

        assert_eq!(line.seq, 7);
        assert_eq!(line.at, "2026-08-15T09:41:02Z");
        assert_eq!(line.stream, Stream::Stderr);
        assert_eq!(line.text, "Error: EADDRINUSE");
    }

    #[test]
    fn each_stream_survives_the_round_trip() {
        for (written, expected) in [
            ("stdout", Stream::Stdout),
            ("stderr", Stream::Stderr),
            ("system", Stream::System),
        ] {
            let line = parse_line(1, &format!("2026-08-15T09:41:02Z [{written}] hello"));
            assert_eq!(line.stream, expected, "for {written}");
            assert_eq!(line.text, "hello");
        }
    }

    /// A line from an older build, or one a project printed oddly, has to
    /// survive. Dropping it would silently hide output the user is looking for.
    #[test]
    fn a_line_that_does_not_match_the_format_is_kept_as_it_is() {
        for raw in [
            "just some text",
            "",
            "2026-08-15 not a timestamp",
            "2026-08-15T09:41:02Z no brackets",
            "2026-08-15T09:41:02Z [unterminated",
        ] {
            let line = parse_line(1, raw);
            assert_eq!(line.text, raw, "dropped or mangled {raw:?}");
        }
    }

    /// Text containing the delimiter must not be truncated at it.
    #[test]
    fn a_message_containing_brackets_is_not_cut_short() {
        let line = parse_line(1, "2026-08-15T09:41:02Z [stdout] [INFO] ready on [::]:3000");
        assert_eq!(line.text, "[INFO] ready on [::]:3000");
    }

    /// Today's file wins over yesterday's, which is the only thing the naming
    /// scheme has to deliver.
    #[test]
    fn the_newest_days_file_is_the_one_read() {
        let root = tempfile::tempdir().expect("temp dir");
        let directory = root.path().join("projects").join("quiet-harbor-4f2a");
        std::fs::create_dir_all(&directory).expect("create");

        for day in ["2026-08-13", "2026-08-15", "2026-08-14"] {
            std::fs::write(directory.join(format!("run-{day}.log")), "x").expect("write");
        }
        // Something that is not a log, to be ignored.
        std::fs::write(directory.join("notes.txt"), "x").expect("write");

        let newest = latest_log_file(root.path(), "quiet-harbor-4f2a").expect("a file");
        assert!(
            newest.ends_with("run-2026-08-15.log"),
            "picked {}",
            newest.display()
        );
    }

    /// A project that has never run has no directory, and that is an empty
    /// console rather than a failure.
    #[test]
    fn a_project_that_never_ran_has_no_log_file() {
        let root = tempfile::tempdir().expect("temp dir");
        assert!(latest_log_file(root.path(), "never-ran").is_none());
    }
}
