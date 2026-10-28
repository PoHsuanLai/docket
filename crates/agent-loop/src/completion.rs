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
    /// The task that started the run is asked again and decides what the person hears: the
    /// answer shows nothing of its own.
    Starter,
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
    /// Why it did not finish, when the run daemon said it by an enumerated kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<docket_core::EndWhy>,
    /// Whether the person is needed.
    pub attention: Attention,
}

/// Whether a report needs the person: a failure does; a normal finish, a cancel and progress
/// do not.
pub fn attention_of(status: ReportStatus) -> Attention {
    match status {
        ReportStatus::Failed => Attention::NeedsYou,
        ReportStatus::Done | ReportStatus::Cancelled | ReportStatus::Progress => Attention::Quiet,
    }
}

/// The one line the front task gets: `the on-screen run finished: Done, 9 steps, 2 values`. It
/// names no id: the line is read by the planner and by the person, and an id is for neither.
/// Progress is not a completion and gets none.
pub fn completion_line(note: &CompletionNote) -> Option<String> {
    let who = match &note.agent {
        AgentRef::Companion => "assistant".to_owned(),
        AgentRef::Worker { .. } => "the background task".to_owned(),
        AgentRef::Cua { .. } => "the on-screen run".to_owned(),
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
    let because = note
        .why
        .map(|why| format!(" ({})", why.words()))
        .unwrap_or_default();
    Some(format!(
        "{who} finished: {how}{because}, {}, {}",
        plural(note.steps.0, "step"),
        plural(note.values.0, "value")
    ))
}
