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

/// How many of the most recent lines are kept in memory, per project.
///
/// Two jobs at two scales. The failure excerpt needs the last few dozen lines;
/// a console someone is watching needs enough history to scroll back through.
/// This is sized for the second, and [`Tail::text`] takes the tail of it for
/// the first.
///
/// Two thousand lines is roughly a quarter of a megabyte per project, so ten
/// projects running at once cost a couple of megabytes — and a project logging
/// in a loop still cannot grow it, which is the property that matters.
const BUFFER_LINES: usize = 2_000;

/// How many lines a failure report quotes.
const EXCERPT_LINES: usize = 50;

/// Which of a project's streams a line came from.
///
/// Kept rather than merged because the interface colours them differently and
/// because "did it write this to stderr" is most of what distinguishes a
/// warning from a crash. `System` is this application's own voice — a start, a
/// stop, a restart — which is neither of the child's streams and must not be
/// mistaken for the project's own output.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stream {
    Stdout,
    Stderr,
    System,
}

impl Stream {
    pub fn as_str(self) -> &'static str {
        match self {
            Stream::Stdout => "stdout",
            Stream::Stderr => "stderr",
            Stream::System => "system",
        }
    }
}

/// One line of a project's output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Monotonic within one project, starting at 1 and never reused.
    ///
    /// This is what lets a console poll for "everything after what I have"
    /// without the two sides having to agree on timestamps, and what makes a
    /// missed poll a gap the next one closes rather than a duplicate.
    pub seq: u64,
    /// When this application received the line, in RFC 3339.
    ///
    /// Received, not produced: a child that buffers its output stamps its lines
    /// with when they arrived here. Saying otherwise would be inventing
    /// precision that does not exist.
    pub at: String,
    pub stream: Stream,
    pub text: String,
}

/// The mutable half, shared between the writer task and every reader.
#[derive(Debug, Default)]
struct Buffer {
    lines: VecDeque<LogLine>,
    /// The sequence number the next line will take. Never reset, so a console
    /// holding an old cursor sees a jump rather than a replay.
    next_seq: u64,
}

/// A project's recent output: the failure excerpt and the live console, in one
/// place.
///
/// The log file is the durable record; this is what can be read back without
/// touching the disk. Both exist because they answer different questions — the
/// file answers "what happened yesterday", this answers "what is it saying
/// right now".
#[derive(Debug, Clone, Default)]
pub struct Tail(Arc<Mutex<Buffer>>);

impl Tail {
    pub fn new() -> Self {
        Self::default()
    }

    /// Lock, recovering from poisoning rather than propagating it.
    ///
    /// A panicked writer must not make a project's output permanently
    /// unreadable — the lines already collected are still true, and losing the
    /// console of every project because one write panicked would be a far
    /// worse outcome than a possibly-truncated buffer.
    fn buffer(&self) -> MutexGuard<'_, Buffer> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn push(&self, stream: Stream, line: &str) {
        let mut buffer = self.buffer();
        if buffer.lines.len() == BUFFER_LINES {
            buffer.lines.pop_front();
        }
        buffer.next_seq += 1;
        let seq = buffer.next_seq;
        buffer.lines.push_back(LogLine {
            seq,
            at: crate::now(),
            stream,
            text: line.to_string(),
        });
    }

    /// Record something this application did, rather than something the project
    /// said.
    ///
    /// Starts, stops, restarts and crash reports go through here, so a console
    /// reads as one story instead of output with unexplained gaps in it.
    pub fn note(&self, line: &str) {
        self.push(Stream::System, line);
    }

    /// Every retained line, oldest first.
    pub fn all(&self) -> Vec<LogLine> {
        self.buffer().lines.iter().cloned().collect()
    }

    /// The lines after `seq`, and the cursor to ask with next time.
    ///
    /// A caller passing `0` gets everything retained, which is what a console
    /// opening for the first time wants.
    pub fn since(&self, seq: u64) -> (Vec<LogLine>, u64) {
        let buffer = self.buffer();
        let lines: Vec<LogLine> = buffer
            .lines
            .iter()
            .filter(|line| line.seq > seq)
            .cloned()
            .collect();
        (lines, buffer.next_seq)
    }

    /// The retained lines as plain text, oldest first.
    pub fn lines_owned(&self) -> Vec<String> {
        self.buffer()
            .lines
            .iter()
            .map(|line| line.text.clone())
            .collect()
    }

    /// The last few lines as one block, or `None` if the child said nothing at
    /// all — which is itself worth distinguishing from a failure that explained
    /// itself.
    ///
    /// Only the project's own output. A failure report quoting this
    /// application's own "starting…" note back at the user would be padding a
    /// message with a line they do not need.
    pub fn text(&self) -> Option<String> {
        let buffer = self.buffer();
        // Collected before reversing: `filter` over a `VecDeque`'s iterator is
        // not `ExactSizeIterator`, so the last-N cannot be taken from the back
        // directly. The buffer is bounded, so this allocation is bounded too.
        let mut lines: Vec<String> = buffer
            .lines
            .iter()
            .filter(|line| line.stream != Stream::System)
            .map(|line| line.text.clone())
            .collect();
        if lines.len() > EXCERPT_LINES {
            lines.drain(..lines.len() - EXCERPT_LINES);
        }
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

/// The pumps running for one child, and the way to know they have finished.
///
/// Held so that a caller reporting a failure can wait for the child's last
/// words to actually be recorded. Reading the tail the instant the child exits
/// races the tasks carrying its output and drops exactly the line that explains
/// what went wrong.
#[derive(Debug)]
pub struct Pumps {
    writer: Option<tokio::task::JoinHandle<()>>,
}

impl Pumps {
    /// Wait until everything the child wrote has been recorded, or until
    /// `limit` passes.
    ///
    /// Bounded, because the writer ends when the pipes close and a child that
    /// left a grandchild holding them never closes them. A failure report that
    /// waited forever would be worse than one missing its last line. Giving up
    /// detaches the writer rather than stopping it, so a late line still
    /// reaches the log file.
    pub async fn drained(&mut self, limit: std::time::Duration) {
        let Some(writer) = self.writer.take() else {
            return;
        };
        let _ = tokio::time::timeout(limit, writer).await;
    }
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
) -> (Tail, Pumps) {
    let tail = Tail::new();
    let pumps = pump_into(stdout, stderr, path, tail.clone());
    (tail, pumps)
}

/// Pump a child's streams into `path`, collecting into an existing tail.
///
/// This is what makes a restart legible. Each attempt is a new child with new
/// pipes, so each needs its own pumps — but a fresh tail per attempt would mean
/// the excerpt shown for the third crash was written by the first child, which
/// stopped talking two failures ago. One tail across the attempts keeps the
/// sequence, which is the interesting thing about a crash loop, and puts the
/// newest lines last where a failure report needs them.
pub fn pump_into(
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    path: PathBuf,
    tail: Tail,
) -> Pumps {
    // Bounded, so a project logging faster than the disk can take it applies
    // back-pressure to itself rather than growing this queue until the
    // application runs out of memory.
    let (sender, receiver) = mpsc::channel::<(Stream, String)>(1024);

    if let Some(stream) = stdout {
        tokio::spawn(read_lines(
            BufReader::new(stream),
            Stream::Stdout,
            sender.clone(),
        ));
    }
    if let Some(stream) = stderr {
        tokio::spawn(read_lines(
            BufReader::new(stream),
            Stream::Stderr,
            sender.clone(),
        ));
    }
    drop(sender);

    Pumps {
        writer: Some(tokio::spawn(write_lines(receiver, path, tail))),
    }
}

/// Read one stream, line by line, into the channel.
async fn read_lines<R>(reader: BufReader<R>, stream: Stream, sender: mpsc::Sender<(Stream, String)>)
where
    R: tokio::io::AsyncRead + Unpin,
{
    let mut lines = reader.lines();
    // A read error ends this pump and nothing else: the other stream may still
    // have something to say, and the child is still the caller's to wait on.
    while let Ok(Some(line)) = lines.next_line().await {
        if sender.send((stream, line)).await.is_err() {
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
async fn write_lines(mut receiver: mpsc::Receiver<(Stream, String)>, path: PathBuf, tail: Tail) {
    if let Some(parent) = path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
        .ok();

    while let Some((stream, line)) = receiver.recv().await {
        tail.push(stream, &line);
        if let Some(file) = file.as_mut() {
            // The file carries the stream and the time as a prefix, because a
            // log read tomorrow with no metadata cannot answer either question,
            // and the in-memory copy that could is long gone.
            let record = format!("{} [{}] {line}", crate::now(), stream.as_str());
            if file.write_all(record.as_bytes()).await.is_err()
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

    /// A project logging in a loop must not be able to grow this without
    /// bound: ten of those running at once is the case the whole cap exists
    /// for.
    #[test]
    fn the_buffer_keeps_only_the_most_recent_lines() {
        let tail = Tail::new();
        for index in 0..BUFFER_LINES + 10 {
            tail.push(Stream::Stdout, &format!("line {index}"));
        }

        let lines = tail.lines_owned();
        assert_eq!(lines.len(), BUFFER_LINES);
        assert_eq!(lines.first().map(String::as_str), Some("line 10"));
        assert_eq!(
            lines.last().map(String::as_str),
            Some(format!("line {}", BUFFER_LINES + 9).as_str())
        );
    }

    /// The failure excerpt is a tail of the buffer, not the whole thing. A
    /// crash report carrying two thousand lines is a crash report nobody reads.
    #[test]
    fn the_failure_excerpt_is_the_last_few_lines_only() {
        let tail = Tail::new();
        for index in 0..EXCERPT_LINES + 20 {
            tail.push(Stream::Stdout, &format!("line {index}"));
        }

        let excerpt = tail.text().unwrap_or_default();
        assert_eq!(excerpt.lines().count(), EXCERPT_LINES);
        assert!(
            excerpt.ends_with(&format!("line {}", EXCERPT_LINES + 19)),
            "the excerpt has to end with what was said most recently"
        );
    }

    #[test]
    fn a_child_that_said_nothing_is_distinguishable_from_one_that_explained_itself() {
        let tail = Tail::new();
        assert_eq!(tail.text(), None);
        tail.push(Stream::Stderr, "Error: listen EADDRINUSE");
        assert_eq!(tail.text().as_deref(), Some("Error: listen EADDRINUSE"));
    }

    /// This application's own notes belong in the console, so a restart reads
    /// as a restart — but not in the failure excerpt, where quoting our own
    /// "starting…" back at the user is padding.
    #[test]
    fn the_applications_own_notes_are_in_the_console_and_not_in_the_excerpt() {
        let tail = Tail::new();
        tail.note("started node (pid 42)");
        tail.push(Stream::Stderr, "TypeError: undefined is not a function");

        assert_eq!(
            tail.text().as_deref(),
            Some("TypeError: undefined is not a function")
        );

        let console = tail.all();
        assert_eq!(console.len(), 2);
        assert_eq!(console[0].stream, Stream::System);
        assert_eq!(console[1].stream, Stream::Stderr);
    }

    /// A console polls with the cursor it last received. The sequence has to
    /// be monotonic for that to work at all, and unique so a poll cannot
    /// deliver a line twice.
    #[test]
    fn a_console_can_ask_for_only_what_it_has_not_seen() {
        let tail = Tail::new();
        tail.push(Stream::Stdout, "first");
        tail.push(Stream::Stdout, "second");

        let (lines, cursor) = tail.since(0);
        assert_eq!(lines.len(), 2);
        assert_eq!(cursor, 2);

        // Nothing new yet.
        let (lines, cursor) = tail.since(cursor);
        assert!(lines.is_empty());
        assert_eq!(cursor, 2);

        tail.push(Stream::Stderr, "third");
        let (lines, cursor) = tail.since(cursor);
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0].text, "third");
        assert_eq!(lines[0].stream, Stream::Stderr);
        assert_eq!(cursor, 3);
    }

    /// The sequence must not restart when the buffer wraps, or a console that
    /// looked away for a moment would be handed lines it already has.
    #[test]
    fn the_sequence_keeps_rising_after_the_buffer_wraps() {
        let tail = Tail::new();
        for index in 0..BUFFER_LINES + 5 {
            tail.push(Stream::Stdout, &format!("line {index}"));
        }

        let (_, cursor) = tail.since(0);
        assert_eq!(cursor as usize, BUFFER_LINES + 5);

        // Asking with a cursor older than anything retained yields what is
        // left rather than nothing, which is the honest answer for a console
        // that fell behind.
        let (lines, _) = tail.since(1);
        assert_eq!(lines.len(), BUFFER_LINES);
    }

    /// Two projects are two buffers. If this were ever shared, five projects
    /// running at once would produce one interleaved console and no way back.
    #[test]
    fn two_projects_do_not_share_a_buffer() {
        let first = Tail::new();
        let second = Tail::new();

        first.push(Stream::Stdout, "from the first project");
        second.push(Stream::Stdout, "from the second project");

        assert_eq!(first.lines_owned(), vec!["from the first project"]);
        assert_eq!(second.lines_owned(), vec!["from the second project"]);
    }

    /// Every line carries when it arrived, or a console cannot show a
    /// timestamp and a log read tomorrow cannot be ordered against anything.
    #[test]
    fn every_line_is_stamped() {
        let tail = Tail::new();
        tail.push(Stream::Stdout, "hello");

        let line = tail.all().pop().expect("a line");
        assert_eq!(line.at.len(), 20, "got {}", line.at);
        assert!(line.at.ends_with('Z'), "got {}", line.at);
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
        let (tail, mut pumps) = pump(child.stdout.take(), child.stderr.take(), path.clone());
        let _ = child.wait().await;

        // The pumps are separate tasks, so the child can exit before its last
        // line has been read. Waiting for them is what makes this deterministic
        // rather than a race that usually goes the right way.
        pumps.drained(std::time::Duration::from_secs(5)).await;

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
        let (tail, mut pumps) = pump(child.stdout.take(), child.stderr.take(), path);
        let _ = child.wait().await;
        pumps.drained(std::time::Duration::from_secs(5)).await;

        assert!(tail.text().unwrap_or_default().contains("EADDRINUSE"));
    }

    /// Successive children of one project share a tail, so the excerpt shown
    /// for a crash loop is the sequence rather than only its first run.
    #[tokio::test]
    async fn attempts_sharing_a_tail_keep_the_whole_sequence() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("run.log");
        let tail = Tail::new();

        for attempt in 1..=3 {
            let mut command;
            #[cfg(windows)]
            {
                command = tokio::process::Command::new("cmd");
                command.args(["/C", &format!("echo attempt {attempt}")]);
            }
            #[cfg(unix)]
            {
                command = tokio::process::Command::new("sh");
                command.args(["-c", &format!("echo attempt {attempt}")]);
            }
            command
                .stdout(std::process::Stdio::piped())
                .stderr(std::process::Stdio::piped());

            let mut child = command.spawn().expect("spawn");
            let mut pumps = pump_into(
                child.stdout.take(),
                child.stderr.take(),
                path.clone(),
                tail.clone(),
            );
            let _ = child.wait().await;
            pumps.drained(std::time::Duration::from_secs(5)).await;
        }

        let captured = tail.text().unwrap_or_default();
        for attempt in 1..=3 {
            assert!(
                captured.contains(&format!("attempt {attempt}")),
                "attempt {attempt} is missing from {captured:?}"
            );
        }
    }

    /// A child whose grandchild holds the pipes open never closes them, so the
    /// wait has to end on its own rather than hanging the failure report.
    #[tokio::test]
    async fn draining_gives_up_rather_than_waiting_on_a_pipe_that_never_closes() {
        let (sender, receiver) = mpsc::channel::<(Stream, String)>(1);
        let directory = tempfile::tempdir().expect("temp dir");
        let mut pumps = Pumps {
            writer: Some(tokio::spawn(write_lines(
                receiver,
                directory.path().join("run.log"),
                Tail::new(),
            ))),
        };

        let started = std::time::Instant::now();
        pumps.drained(std::time::Duration::from_millis(200)).await;
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "draining waited on a stream that was never going to close"
        );

        // Giving up detaches the writer rather than stopping it.
        drop(sender);
    }
}
