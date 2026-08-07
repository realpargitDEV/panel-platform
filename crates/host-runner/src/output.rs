//! Getting a child's output somewhere it can be read after the child is gone.
//!
//! A container's output is retained by the daemon: the project dies, and
//! `docker logs` still answers. A pipe's output is gone the moment the process
//! holding the other end is. So a host project that fails during startup would
//! otherwise leave `FAILED` and nothing whatsoever to read, which is why log
//! capture is in the first version rather than a later one.
//!
//! Two streams, one file, one writer. `stdout` and `stderr` are pumped by
//! separate tasks into a channel, and a single task drains it. Interleaving
//! through a channel is what keeps the two streams in the order they were
//! actually produced; two tasks writing to one file directly would not.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::sync::mpsc;

/// How many of the most recent lines are kept in memory.
///
/// Enough to carry a stack trace or a port-in-use message into a failure
/// report, and few enough that a project logging in a loop cannot grow this
/// without bound.
const TAIL_LINES: usize = 50;

/// The last lines a child produced, for the message shown when it fails.
///
/// The log file is the record; this is the excerpt. A failure the user has to
/// go and open a file to understand is a failure reported badly.
#[derive(Debug, Clone, Default)]
pub struct Tail(Arc<Mutex<VecDeque<String>>>);

impl Tail {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock, recovering from poisoning rather than propagating it.
    ///
    /// A panicked writer must not make a project's output permanently
    /// unreadable — the lines already collected are still true, and the tail
    /// is only ever used to explain a failure that has already happened.
    fn lines(&self) -> MutexGuard<'_, VecDeque<String>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push(&self, line: &str) {
        let mut lines = self.lines();
        if lines.len() == TAIL_LINES {
            lines.pop_front();
        }
        lines.push_back(line.to_string());
    }

    /// The retained lines, oldest first.
    pub fn lines_owned(&self) -> Vec<String> {
        self.lines().iter().cloned().collect()
    }

    /// The retained lines as one block of text, or `None` if the child said
    /// nothing at all — which is itself worth distinguishing from a failure
    /// that explained itself.
    pub fn text(&self) -> Option<String> {
        let lines = self.lines_owned();
        (!lines.is_empty()).then(|| lines.join("\n"))
    }
}

/// Where a project's output for this run is written.
///
/// One file per project per day. Not per run: a project that crash-loops would
/// otherwise produce a file per attempt, and the interesting thing about a
/// crash loop is the sequence.
pub fn log_path(logs_root: &Path, slug: &str, today: &str) -> PathBuf {
    logs_root
        .join("projects")
        .join(slug)
        .join(format!("run-{today}.log"))
}

/// Start pumping a child's streams into `path`, and answer with the tail.
///
/// Returns as soon as the pumps are running; the child's output arrives on its
/// own schedule. The writer task ends when both streams close, which is when
/// the child and everything sharing its handles have exited.
pub fn pump(
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    path: PathBuf,
) -> Tail {
    let tail = Tail::new();
    // Bounded, so a project logging faster than the disk can take it applies
    // back-pressure to itself rather than growing this queue until the
    // application runs out of memory.
    let (sender, receiver) = mpsc::channel::<String>(1024);

    if let Some(stream) = stdout {
        tokio::spawn(read_lines(BufReader::new(stream), sender.clone()));
    }
    if let Some(stream) = stderr {
        tokio::spawn(read_lines(BufReader::new(stream), sender.clone()));
    }
    drop(sender);

    tokio::spawn(write_lines(receiver, path, tail.clone()));
    tail
}

/// Read one stream, line by line, into the channel.
async fn read_lines<R>(reader: BufReader<R>, sender: mpsc::Sender<String>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = reader.lines();
    // A read error ends this pump and nothing else: the other stream may still
    // have something to say, and the child is still the caller's to wait on.
    while let Ok(Some(line)) = lines.next_line().await {
        if sender.send(line).await.is_err() {
            return;
        }
    }
}

/// Drain the channel into the log file, keeping the tail up to date.
///
/// A file that cannot be opened is not fatal. The tail still fills, so the
/// failure message still has something in it, and the project still runs —
/// refusing to run a project because its log file could not be created would
/// be the wrong trade in both directions.
async fn write_lines(mut receiver: mpsc::Receiver<String>, path: PathBuf, tail: Tail) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .ok();

    while let Some(line) = receiver.recv().await {
        tail.push(&line);
        if let Some(file) = file.as_mut() {
            if file.write_all(line.as_bytes()).await.is_err()
                || file.write_all(b"\n").await.is_err()
            {
                // The disk filled, or the file was removed underneath us.
                // Stop trying; the tail keeps working.
                continue;
            }
        }
    }

    if let Some(file) = file.as_mut() {
        let _ = file.flush().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_tail_keeps_only_the_most_recent_lines() {
        let tail = Tail::new();
        for index in 0..TAIL_LINES + 10 {
            tail.push(&format!("line {index}"));
        }

        let lines = tail.lines_owned();
        assert_eq!(lines.len(), TAIL_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 10"));
        assert_eq!(
            lines.last().map(String::as_str),
            Some(format!("line {}", TAIL_LINES + 9).as_str())
        );
    }

    #[test]
    fn a_child_that_said_nothing_is_distinguishable_from_one_that_explained_itself() {
        let tail = Tail::new();
        assert_eq!(tail.text(), None);
        tail.push("Error: listen EADDRINUSE");
        assert_eq!(tail.text().as_deref(), Some("Error: listen EADDRINUSE"));
    }

    #[test]
    fn a_projects_log_is_one_file_per_day_under_its_own_directory() {
        let path = log_path(Path::new("/logs"), "quiet-harbor-4f2a", "2026-08-07");
        assert!(path.ends_with("projects/quiet-harbor-4f2a/run-2026-08-07.log"));
    }

    /// The property the whole module exists for: a child that writes and then
    /// dies leaves its words behind.
    #[tokio::test]
    async fn what_a_child_printed_survives_the_child() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("run.log");

        let mut command;
        #[cfg(windows)]
        {
            command = tokio::process::Command::new("cmd");
            command.args(["/C", "echo hello from the child"]);
        }
        #[cfg(unix)]
        {
            command = tokio::process::Command::new("sh");
            command.args(["-c", "echo hello from the child"]);
        }
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn().expect("spawn");
        let tail = pump(child.stdout.take(), child.stderr.take(), path.clone());
        let _ = child.wait().await;

        // The pumps are separate tasks; give them a moment to drain.
        for _ in 0..100 {
            if tail.text().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        let captured = tail.text().unwrap_or_default();
        assert!(
            captured.contains("hello from the child"),
            "the tail held {captured:?}"
        );

        let written = tokio::fs::read_to_string(&path).await.unwrap_or_default();
        assert!(
            written.contains("hello from the child"),
            "the log file held {written:?}"
        );
    }

    /// stderr matters more than stdout here: it is where a start failure
    /// explains itself.
    #[tokio::test]
    async fn standard_error_is_captured_too() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("run.log");

        let mut command;
        #[cfg(windows)]
        {
            command = tokio::process::Command::new("cmd");
            command.args(["/C", "echo EADDRINUSE 1>&2"]);
        }
        #[cfg(unix)]
        {
            command = tokio::process::Command::new("sh");
            command.args(["-c", "echo EADDRINUSE 1>&2"]);
        }
        command
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());

        let mut child = command.spawn().expect("spawn");
        let tail = pump(child.stdout.take(), child.stderr.take(), path);
        let _ = child.wait().await;

        for _ in 0..100 {
            if tail.text().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        assert!(tail.text().unwrap_or_default().contains("EADDRINUSE"));
    }
}
