//! The processes `up` started, one line each in `<root>/procs`: `name pid start`, where `start`
//! is field 22 of `/proc/<pid>/stat` (the process's start time in clock ticks since boot). A pid
//! is only ever signalled while its start time still matches, so a pid the system reused for
//! another program is never touched.

use std::path::{Path, PathBuf};

/// The file's place in a root.
pub fn file(root: &Path) -> PathBuf {
    root.join("procs")
}

/// One started process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record {
    /// The daemon's name.
    pub name: String,
    /// Its process id.
    pub pid: u32,
    /// Its start time.
    pub start: u64,
}

/// Whether a recorded process is still the one that was started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// The pid is alive and has the recorded start time.
    Running,
    /// The pid is gone, a zombie, or belongs to another process now.
    Gone,
}

impl State {
    /// The word `status` prints.
    pub fn word(self) -> &'static str {
        match self {
            State::Running => "running",
            State::Gone => "gone",
        }
    }
}

/// The state letter and start time out of the text of a `/proc/<pid>/stat`. The command name is
/// in parentheses and may hold spaces and parentheses, so the fields are counted after the LAST
/// closing parenthesis: the state is field 3, the start time field 22.
pub fn parse_stat(text: &str) -> Option<(char, u64)> {
    let after = text.rsplit_once(')')?.1;
    let mut fields = after.split_whitespace();
    let state = fields.next()?.chars().next()?;
    let start = fields.nth(18)?.parse().ok()?;
    Some((state, start))
}

fn stat_text(pid: u32) -> Option<String> {
    std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()
}

/// The start time of `pid` now.
pub fn start_time(pid: u32) -> Option<u64> {
    stat_text(pid).as_deref().and_then(parse_stat).map(|s| s.1)
}

impl Record {
    /// The state this record is in, given the text of its pid's `stat` (none: no such process).
    pub fn state_in(&self, stat: Option<&str>) -> State {
        match stat.and_then(parse_stat) {
            Some((letter, start)) if start == self.start && !matches!(letter, 'Z' | 'X') => {
                State::Running
            }
            _ => State::Gone,
        }
    }

    /// The state now.
    pub fn state(&self) -> State {
        self.state_in(stat_text(self.pid).as_deref())
    }
}

/// Why the file cannot be read.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum ProcsFault {
    /// A line is not `name pid start`.
    #[error("a line of the procs file is not `name pid start`: {0:?}")]
    Line(String),
    /// The file cannot be read.
    #[error("{0}: {1}")]
    Io(PathBuf, String),
}

/// The text of the file for `records`.
pub fn render(records: &[Record]) -> String {
    records
        .iter()
        .map(|r| format!("{} {} {}\n", r.name, r.pid, r.start))
        .collect()
}

/// The records in `text`.
pub fn parse(text: &str) -> Result<Vec<Record>, ProcsFault> {
    let line = |line: &str| {
        let mut words = line.split_whitespace();
        let record = (|| {
            Some(Record {
                name: words.next()?.to_owned(),
                pid: words.next()?.parse().ok()?,
                start: words.next()?.parse().ok()?,
            })
        })();
        record
            .filter(|_| words.next().is_none())
            .ok_or_else(|| ProcsFault::Line(line.to_owned()))
    };
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(line)
        .collect()
}

/// The records of `root` (none when it has no file).
pub fn read(root: &Path) -> Result<Vec<Record>, ProcsFault> {
    let path = file(root);
    match std::fs::read_to_string(&path) {
        Ok(text) => parse(&text),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(ProcsFault::Io(path, e.to_string())),
    }
}

/// Replaces the file of `root` with `records`.
pub fn write(root: &Path, records: &[Record]) -> std::io::Result<()> {
    std::fs::write(file(root), render(records))
}
