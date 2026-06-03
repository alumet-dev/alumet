//! Captures the process's standard error while the interactive UI is up.
//!
//! Alumet logs through `env_logger`, which writes to stderr. Because stderr and the TUI (drawn on
//! stdout's alternate screen) share the same physical terminal, those log lines would bleed over
//! the display. To avoid that — without losing the logs — we redirect the stderr file descriptor to
//! a pipe, read it on a background thread, parse each line into a [`LogEntry`], and keep the most
//! recent entries in a bounded ring buffer that the TUI renders as a scrollable, sortable and
//! filterable table. The original stderr is restored when the capture is dropped.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// One captured log line, parsed from `env_logger`'s default format
/// `[<timestamp> <LEVEL> <target>] <message>`. Lines that do not match (continuation lines of a
/// multi-line message, panics, raw writes) are folded into the previous entry's message, so a
/// stored entry always carries a real timestamp and level — except a leading unparsable line, which
/// is kept verbatim in `message` with an empty `time` and no `level`.
#[derive(Clone, Debug)]
pub struct LogEntry {
    /// RFC3339 timestamp (UTC, e.g. `2026-06-03T14:22:01Z`), or empty if the line was not parsed.
    pub time: String,
    /// Severity, or `None` for an unparsed line.
    pub level: Option<log::Level>,
    /// The log target (usually the module path), or empty if unknown.
    pub module: String,
    /// The message text (continuation lines are appended, separated by a space).
    pub message: String,
}

/// A bounded ring of recent [`LogEntry`]s. The oldest entry is dropped once `capacity` is reached,
/// so memory stays bounded regardless of how chatty the sources are.
pub struct LogStore {
    entries: VecDeque<LogEntry>,
    capacity: usize,
}

impl LogStore {
    fn new(capacity: usize) -> Self {
        // Grow lazily up to `capacity` rather than allocating it all up front, so a generous cap is
        // cheap until it actually fills.
        Self {
            entries: VecDeque::with_capacity(capacity.min(4096)),
            capacity,
        }
    }

    /// The stored entries, oldest first.
    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    /// Ingests one captured line: a parsable line becomes a new entry (evicting the oldest when
    /// full); an unparsable line is appended to the previous entry, keeping multi-line messages
    /// together rather than scattering timestamp-less rows that would break sorting.
    fn push(&mut self, line: String) {
        let entry = parse_line(&line);
        let is_continuation = entry.time.is_empty() && entry.level.is_none();
        if is_continuation && let Some(last) = self.entries.back_mut() {
            last.message.push(' ');
            last.message.push_str(&entry.message);
            return;
        }
        if self.entries.len() >= self.capacity {
            self.entries.pop_front();
        }
        self.entries.push_back(entry);
    }
}

/// Shared, bounded ring buffer of recent log entries.
pub type LogBuffer = Arc<Mutex<LogStore>>;

/// Creates a log buffer holding at most `capacity` entries (clamped to at least 1).
pub fn new_buffer(capacity: usize) -> LogBuffer {
    Arc::new(Mutex::new(LogStore::new(capacity.max(1))))
}

/// Parses one `env_logger`-formatted line into a [`LogEntry`]. The default format is
/// `[<timestamp> <LEVEL> <target>] <message>`; anything that does not match is returned with an
/// empty `time`/`level` and the whole line as `message` (treated as a continuation by [`LogStore`]).
fn parse_line(raw: &str) -> LogEntry {
    if let Some(rest) = raw.strip_prefix('[')
        && let Some((head, msg)) = rest.split_once(']')
    {
        let mut fields = head.split_whitespace();
        if let (Some(ts), Some(lvl)) = (fields.next(), fields.next())
            && let Some(level) = parse_level(lvl)
        {
            return LogEntry {
                time: ts.to_string(),
                level: Some(level),
                module: fields.next().unwrap_or("").to_string(),
                message: msg.strip_prefix(' ').unwrap_or(msg).to_string(),
            };
        }
    }
    LogEntry {
        time: String::new(),
        level: None,
        module: String::new(),
        message: raw.to_string(),
    }
}

/// Maps `env_logger`'s level word to a [`log::Level`].
fn parse_level(s: &str) -> Option<log::Level> {
    match s {
        "ERROR" => Some(log::Level::Error),
        "WARN" => Some(log::Level::Warn),
        "INFO" => Some(log::Level::Info),
        "DEBUG" => Some(log::Level::Debug),
        "TRACE" => Some(log::Level::Trace),
        _ => None,
    }
}

/// Pushes a captured line into the store.
fn push_line(logs: &LogBuffer, line: String) {
    let mut store = logs.lock().expect("log buffer mutex poisoned");
    store.push(line);
}

#[cfg(unix)]
mod imp {
    use super::{LogBuffer, push_line};
    use std::fs::File;
    use std::io::{BufRead, BufReader};
    use std::os::fd::FromRawFd;
    use std::thread::JoinHandle;

    /// Redirects stderr to a pipe for its lifetime; restores it on drop.
    pub struct StderrCapture {
        /// A dup of the original stderr, restored onto fd 2 on drop.
        original_fd: i32,
        /// Our own copy of the pipe's write end; closing it lets the reader thread reach EOF.
        write_fd: i32,
        reader: Option<JoinHandle<()>>,
    }

    impl StderrCapture {
        pub fn start(logs: LogBuffer) -> Option<Self> {
            // Save the current stderr and build a pipe.
            let original_fd = unsafe { libc::dup(libc::STDERR_FILENO) };
            if original_fd < 0 {
                return None;
            }
            let mut fds = [0i32; 2];
            if unsafe { libc::pipe(fds.as_mut_ptr()) } != 0 {
                unsafe { libc::close(original_fd) };
                return None;
            }
            let (read_fd, write_fd) = (fds[0], fds[1]);

            // Point stderr at the pipe's write end.
            if unsafe { libc::dup2(write_fd, libc::STDERR_FILENO) } < 0 {
                unsafe {
                    libc::close(read_fd);
                    libc::close(write_fd);
                    libc::close(original_fd);
                }
                return None;
            }

            // Drain the pipe on a background thread.
            let reader = std::thread::Builder::new()
                .name("tui-logcap".to_owned())
                .spawn(move || {
                    let file = unsafe { File::from_raw_fd(read_fd) };
                    let reader = BufReader::new(file);
                    for line in reader.lines() {
                        match line {
                            Ok(line) => push_line(&logs, strip_ansi(&line)),
                            Err(_) => break,
                        }
                    }
                })
                .ok();

            Some(Self {
                original_fd,
                write_fd,
                reader,
            })
        }
    }

    impl Drop for StderrCapture {
        fn drop(&mut self) {
            unsafe {
                // Restore the real stderr, then close the pipe so the reader thread reaches EOF.
                libc::dup2(self.original_fd, libc::STDERR_FILENO);
                libc::close(self.original_fd);
                libc::close(self.write_fd);
            }
            if let Some(reader) = self.reader.take() {
                let _ = reader.join();
            }
        }
    }

    /// Removes ANSI escape sequences so captured lines render cleanly in the TUI.
    fn strip_ansi(s: &str) -> String {
        let mut out = String::with_capacity(s.len());
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            if c == '\x1b' {
                // Skip a CSI sequence: ESC '[' ... final byte in 0x40..=0x7e.
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('\x40'..='\x7e').contains(&c) {
                            break;
                        }
                    }
                }
            } else {
                out.push(c);
            }
        }
        out
    }
}

#[cfg(not(unix))]
mod imp {
    use super::LogBuffer;

    /// No-op on non-unix platforms (no fd redirection available).
    pub struct StderrCapture;

    impl StderrCapture {
        pub fn start(_logs: LogBuffer) -> Option<Self> {
            None
        }
    }
}

pub use imp::StderrCapture;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_the_default_env_logger_format() {
        let e = parse_line("[2026-06-03T14:22:01Z INFO  alumet::pipeline] started 3 sources");
        assert_eq!(e.time, "2026-06-03T14:22:01Z");
        assert_eq!(e.level, Some(log::Level::Info));
        assert_eq!(e.module, "alumet::pipeline");
        assert_eq!(e.message, "started 3 sources");
    }

    #[test]
    fn unparsable_line_is_kept_whole_with_no_level() {
        let e = parse_line("thread 'main' panicked at src/lib.rs:42");
        assert!(e.time.is_empty());
        assert_eq!(e.level, None);
        assert_eq!(e.message, "thread 'main' panicked at src/lib.rs:42");
    }

    #[test]
    fn continuation_lines_fold_into_the_previous_entry() {
        let mut store = LogStore::new(10);
        store.push("[2026-06-03T14:22:01Z ERROR plugin] boom".to_string());
        store.push("  caused by: io error".to_string());
        assert_eq!(store.entries().len(), 1);
        assert_eq!(store.entries()[0].message, "boom   caused by: io error");
    }

    #[test]
    fn the_oldest_entry_is_evicted_when_full() {
        let mut store = LogStore::new(2);
        for i in 0..4 {
            store.push(format!("[2026-06-03T14:22:0{i}Z INFO m] line {i}"));
        }
        assert_eq!(store.entries().len(), 2);
        assert_eq!(store.entries()[0].message, "line 2");
        assert_eq!(store.entries()[1].message, "line 3");
    }
}
