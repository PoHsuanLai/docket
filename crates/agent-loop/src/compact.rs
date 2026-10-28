//! Compaction, the pure half: when a task has outgrown its budget, which of its oldest turns and
//! steps a summary should stand for, and what the summariser may be shown. The model turn that
//! writes the summary and the log entry that records it are the loop's (`docket-planner`,
//! `docket-tasks`); nothing here asks a model or writes anything.
//!
//! Three rules keep a summary from becoming a way in for untrusted text:
//! - the input is built only from what the planner already saw: the person's own turns, the
//!   planner's steps masked as the assembler masks them (a value that is not a handle is dropped),
//!   and the handles it holds. No raw app result is ever in it;
//! - a handle the summary cites stays a handle (`SummaryText::parse` refuses one the planner never
//!   held);
//! - the summary is as untrusted as the task was when it was written, and never more trusted.

use crate::assemble::{Sources, mask_history, task_cost, view_of};
use docket_core::{
    AssemblerBudget, Covers, Reveal, StepEnd, StepLine, Summary, SummaryText, UserTurn,
};
use docket_vocab::{Handle, Value};
use porter_core::{Count, Tokens};
use prov::Integrity;
use serde::{Deserialize, Serialize};

/// Tokens set aside, when deciding what to cover, for the summary that will stand in its place.
pub const SUMMARY_RESERVE: Tokens = Tokens(500);

fn count(n: usize) -> Count {
    Count(u32::try_from(n).unwrap_or(u32::MAX))
}

fn covered_now(sources: &Sources) -> Covers {
    sources.summary.as_ref().map_or(Covers::NONE, |s| s.covers)
}

fn fits_with(budget: &AssemblerBudget, sources: &Sources, covers: Covers) -> bool {
    let view = view_of(budget, sources, covers, None);
    task_cost(&view).saturating_add(SUMMARY_RESERVE.0) <= budget.task.0
}

/// What a summary should newly stand for, when the task section is over its budget: the oldest
/// whole steps (a step is a call and its result, never split), as few as make the rest fit beside
/// a summary, never the newest `full_steps`. Only when every step that may be covered is not
/// enough are older turns covered too, and the newest turn is never covered. `None` when the task
/// fits, or when there is nothing left to cover (the loop then splits the task).
pub fn compaction_due(budget: &AssemblerBudget, sources: &Sources) -> Option<Covers> {
    let now = covered_now(sources);
    let shown = view_of(budget, sources, now, sources.summary.clone());
    if task_cost(&shown) <= budget.task.0 {
        return None;
    }
    let (turns_from, steps_from) = (now.turns.0 as usize, now.steps.0 as usize);
    let steps_to = sources
        .history
        .len()
        .saturating_sub(budget.full_steps.0 as usize);
    let turns_to = sources.turns.len().saturating_sub(1);
    let by_steps = (steps_from + 1..=steps_to).map(|k| Covers {
        turns: now.turns,
        steps: count(k),
    });
    let all_steps = count(steps_to.max(steps_from));
    let by_turns = (turns_from + 1..=turns_to).map(|t| Covers {
        turns: count(t),
        steps: all_steps,
    });
    let candidates: Vec<Covers> = by_steps.chain(by_turns).collect();
    candidates
        .iter()
        .find(|c| fits_with(budget, sources, **c))
        .or(candidates.last())
        .copied()
}

/// What the summariser is shown, and what it may cite. Built by [`summary_input`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SummaryInput {
    /// The summary already in place, to be folded into the new one.
    pub earlier: Option<Summary>,
    /// The person's turns newly covered, oldest first.
    pub turns: Vec<UserTurn>,
    /// The steps newly covered, oldest first, masked as the assembler masks them.
    pub steps: Vec<StepLine>,
    /// The handles the planner held: the only ones a summary may cite.
    pub held: Vec<Handle>,
    /// What the new summary will stand for in all.
    pub covers: Covers,
    /// The task's integrity now: the summary will have no more.
    pub integrity: Integrity,
}

fn handles_in(value: &Reveal<Value>) -> Vec<Handle> {
    match value {
        Reveal::Handle(h) | Reveal::Plain(Value::Handle(h)) => vec![*h],
        Reveal::Plain(Value::List(items)) => items
            .iter()
            .filter_map(|i| match i {
                Value::Handle(h) => Some(*h),
                _ => None,
            })
            .collect(),
        Reveal::Plain(_) => Vec::new(),
    }
}

fn handles_of(step: &StepLine) -> Vec<Handle> {
    let returned = match &step.end {
        StepEnd::Done {
            value: Some(value), ..
        } => handles_in(value),
        _ => Vec::new(),
    };
    step.with.iter().copied().chain(returned).collect()
}

/// The input for a summary that stands for `covers` (as [`compaction_due`] chose it).
pub fn summary_input(sources: &Sources, covers: Covers) -> SummaryInput {
    let now = covered_now(sources);
    let (turns_from, steps_from) = (now.turns.0 as usize, now.steps.0 as usize);
    let turns = sources
        .turns
        .get(turns_from..covers.turns.0 as usize)
        .unwrap_or_default();
    let steps = sources
        .history
        .get(steps_from..covers.steps.0 as usize)
        .unwrap_or_default();
    let mut held: Vec<Handle> = sources.handles.iter().map(|c| c.handle).collect();
    held.extend(sources.history.iter().flat_map(handles_of));
    held.sort();
    held.dedup();
    SummaryInput {
        earlier: sources.summary.clone(),
        turns: turns.to_vec(),
        steps: mask_history(steps, Count(0)),
        held,
        covers,
        integrity: sources.taint,
    }
}

/// The summary `text` becomes once it stands for `input.covers`: as untrusted as the task is.
pub fn summarised(input: &SummaryInput, text: SummaryText) -> Summary {
    Summary {
        covers: input.covers,
        text,
        integrity: input.integrity,
    }
}
