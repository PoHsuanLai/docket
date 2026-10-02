//! A worker or run reports `Done`, `Failed` or `Cancelled`: the front thread gets one line in
//! its current task (like a completion notification), and the orb shows Waiting only when the
//! result needs the person. The line is built from typed facts, never from what the worker said.

use porter_core::Count;
use prov::{AgentRef, ReportStatus};
use serde::{Deserialize, Serialize};

/// Whether the person is needed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Attention {
    /// Nothing for them to do.
    Quiet,
    /// The orb shows Waiting.
    NeedsYou,
}

/// Whether the report asked the person something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AskedPerson {
    /// It did.
    Yes,
    /// It did not.
    No,
}

/// A final report, as typed facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletionNote {
    /// Who reported.
    pub agent: AgentRef,
    /// How it ended.
    pub status: ReportStatus,
    /// How many steps it took.
    pub steps: Count,
    /// How many values it returned.
    pub values: Count,
    /// Whether the person is needed.
    pub attention: Attention,
}

/// Whether a report needs the person: a failure does, and so does anything that asked them
/// something; a normal finish, a cancel and progress do not.
pub fn attention_of(status: ReportStatus, asked: AskedPerson) -> Attention {
    match (status, asked) {
        (_, AskedPerson::Yes) | (ReportStatus::Failed, AskedPerson::No) => Attention::NeedsYou,
        (
            ReportStatus::Done | ReportStatus::Cancelled | ReportStatus::Progress,
            AskedPerson::No,
        ) => Attention::Quiet,
    }
}

/// The one line the front task gets: `run r-3 finished: Done, 9 steps, 2 values`. Progress is
/// not a completion and gets none.
pub fn completion_line(note: &CompletionNote) -> Option<String> {
    let who = match &note.agent {
        AgentRef::Companion => "companion".to_owned(),
        AgentRef::Worker { task } => format!("task {task}"),
        AgentRef::Cua { run } => format!("run {run}"),
        AgentRef::User => "you".to_owned(),
    };
    let how = match note.status {
        ReportStatus::Done => "Done",
        ReportStatus::Failed => "Failed",
        ReportStatus::Cancelled => "Cancelled",
        ReportStatus::Progress => return None,
    };
    let plural = |n: u32, word: &str| {
        if n == 1 {
            format!("{n} {word}")
        } else {
            format!("{n} {word}s")
        }
    };
    Some(format!(
        "{who} finished: {how}, {}, {}",
        plural(note.steps.0, "step"),
        plural(note.values.0, "value")
    ))
}
