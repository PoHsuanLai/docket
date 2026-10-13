//! Stopping what `up` started, and saying what is still running. Only the pids in `<root>/procs`
//! whose start time still matches are signalled; nothing is found by name.

use super::procs::{self, ProcsFault, Record, State};
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// How long a daemon gets after SIGTERM before SIGKILL.
const GRACE: Duration = Duration::from_secs(5);
const POLL: Duration = Duration::from_millis(100);

/// How a recorded process ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// It was not running any more.
    AlreadyGone,
    /// It exited after SIGTERM.
    Stopped,
    /// It had to be killed.
    Killed,
}

impl Outcome {
    /// The word `down` prints.
    pub fn word(self) -> &'static str {
        match self {
            Outcome::AlreadyGone => "gone",
            Outcome::Stopped => "stopped",
            Outcome::Killed => "killed",
        }
    }
}

/// A recorded process and how it ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stopped {
    /// The record.
    pub record: Record,
    /// What happened to it.
    pub outcome: Outcome,
}

/// Sends `signal` to `pid`, which the caller has just checked is the recorded process.
fn signal(pid: u32, signal: &str) {
    let _ = Command::new("kill")
        .args(["-s", signal])
        .arg(pid.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

fn gone_within(record: &Record, bound: Duration) -> bool {
    let end = Instant::now() + bound;
    while Instant::now() < end {
        if record.state() == State::Gone {
            return true;
        }
        std::thread::sleep(POLL);
    }
    record.state() == State::Gone
}

/// Stops one recorded process: SIGTERM, then SIGKILL after the grace period, each only while the
/// pid still has the recorded start time.
pub fn stop(record: &Record) -> Outcome {
    if record.state() == State::Gone {
        return Outcome::AlreadyGone;
    }
    signal(record.pid, "TERM");
    if gone_within(record, GRACE) {
        return Outcome::Stopped;
    }
    if record.state() == State::Running {
        signal(record.pid, "KILL");
        gone_within(record, GRACE);
    }
    Outcome::Killed
}

/// Stops every process `root` recorded, the last started first. The root stays on disk.
pub fn down(root: &Path) -> Result<Vec<Stopped>, ProcsFault> {
    let records = procs::read(root)?;
    Ok(records
        .into_iter()
        .rev()
        .map(|record| {
            let outcome = stop(&record);
            Stopped { record, outcome }
        })
        .collect())
}

/// Every recorded process and whether it is still the one that was started.
pub fn status(root: &Path) -> Result<Vec<(Record, State)>, ProcsFault> {
    Ok(procs::read(root)?
        .into_iter()
        .map(|record| {
            let state = record.state();
            (record, state)
        })
        .collect())
}
