//! The working-set assembler: one function from what the router built to the planner's prompt
//! sections, inside a fixed token budget. Sections run from most stable to least stable so the
//! engine reuses its cached prefix: rules and action cards, then profile and primer, the roster,
//! recent episodes, recall, the context, and last the current task.
//!
//! Nothing here reads a clock or an order that changes between turns, so the first sections are
//! byte-stable within a day. Compression is deterministic: masking old outcomes, and dropping the
//! oldest masked steps; the model is never asked to summarise mid-task. A task that still
//! overflows is split by the loop (finish or hand off), not squeezed.

use almanac_core::{estimate_tokens, fit_budget};
use docket_core::{
    ActionCard, AssemblerBudget, ContextView, EpisodeLine, HandleCard, InboundLine, PlannerView,
    PrimerText, ProfileLine, RecalledLine, RollupLine, Roster, StepEnd, StepLine, StepShown,
    TaskPolicy, UserTurn,
};
use porter_core::{Count, Tokens};
use prov::Integrity;
use serde::{Deserialize, Serialize};

/// What the router built for one turn, before the budget is applied.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Sources {
    /// 1: the actions the Space offers, most relevant first.
    pub cards: Vec<ActionCard>,
    /// 2: pinned desktop-scope facts.
    pub profile: Vec<ProfileLine>,
    /// 2: the Space's primer.
    pub primer: Option<PrimerText>,
    /// 2: the latest digest line.
    pub rollup: Option<RollupLine>,
    /// 3: who else is working.
    pub roster: Roster,
    /// 4: recent episodes, newest first.
    pub episodes: Vec<EpisodeLine>,
    /// 5: recall, best first.
    pub recalled: Vec<RecalledLine>,
    /// 6: where the person is.
    pub context: ContextView,
    /// 7: the person's turns in this task.
    pub turns: Vec<UserTurn>,
    /// 7: the task's calls, oldest first.
    pub history: Vec<StepLine>,
    /// 7: the handles the planner may name.
    pub handles: Vec<HandleCard>,
    /// 7: messages that landed.
    pub inbox: Vec<InboundLine>,
    /// The planner's integrity as the router computed it.
    pub taint: Integrity,
    /// What the task may do.
    pub task_policy: Option<TaskPolicy>,
}

fn cost<T: Serialize>(value: &T) -> Tokens {
    // Every type here serialises; the empty string only stands in for a failure that cannot
    // happen, and costs nothing.
    estimate_tokens(&serde_json::to_string(value).unwrap_or_default())
}

/// The items that fit, in order, stopping at the first that does not (time-ordered lists: what
/// does not fit is older or less relevant than what came before).
fn take_within<T: Serialize + Clone>(items: &[T], budget: Tokens) -> Vec<T> {
    let mut spent = 0u32;
    let mut taken = Vec::new();
    for item in items {
        match spent.checked_add(cost(item).0) {
            Some(next) if next <= budget.0 => {
                spent = next;
                taken.push(item.clone());
            }
            _ => break,
        }
    }
    taken
}

/// The history with every step but the newest `keep_full` masked: the value is dropped, and
/// the line keeps what the app said and the undo row, so the cached prefix stays stable until
/// the task ends.
pub fn mask_history(history: &[StepLine], keep_full: Count) -> Vec<StepLine> {
    let cut = history.len().saturating_sub(keep_full.0 as usize);
    history
        .iter()
        .enumerate()
        .map(|(i, step)| {
            if i >= cut {
                return step.clone();
            }
            let end = match &step.end {
                StepEnd::Done { said, undo, .. } => StepEnd::Done {
                    said: said.clone(),
                    value: None,
                    undo: *undo,
                },
                other => other.clone(),
            };
            StepLine {
                end,
                shown: StepShown::Masked,
                ..step.clone()
            }
        })
        .collect()
}

/// Whether the current task's section fits its budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskFit {
    /// It does.
    Fits,
    /// It does not, even with everything old dropped: the loop splits the task.
    Overflows,
}

fn task_cost(view: &PlannerView) -> u32 {
    cost(&view.turns).0 + cost(&view.history).0 + cost(&view.inbox).0 + cost(&view.handles).0
}

/// Whether the task section of `view` is within the budget.
pub fn task_fit(budget: &AssemblerBudget, view: &PlannerView) -> TaskFit {
    if task_cost(view) <= budget.task.0 {
        TaskFit::Fits
    } else {
        TaskFit::Overflows
    }
}

/// Builds the planner's view inside the budget. The person's turns are never cut; the oldest
/// masked steps go first when the current task is over its budget.
pub fn assemble(budget: &AssemblerBudget, sources: &Sources) -> PlannerView {
    let primer_room = Tokens(budget.profile.0.saturating_sub(cost(&sources.profile).0));
    let mut context = sources.context.clone();
    while cost(&context).0 > budget.context.0 && !context.visible.items.is_empty() {
        context.visible.items.pop();
    }
    let recalled = fit_budget(
        sources.recalled.clone(),
        Count(u32::MAX),
        budget.recall,
        cost,
    );
    let mut view = PlannerView {
        turns: sources.turns.clone(),
        context,
        actions: take_within(&sources.cards, budget.rules),
        handles: sources.handles.clone(),
        history: mask_history(&sources.history, budget.full_steps),
        taint: sources.taint,
        task_policy: sources.task_policy.clone(),
        primer: sources
            .primer
            .clone()
            .filter(|p| cost(p).0 <= primer_room.0),
        profile: take_within(&sources.profile, budget.profile),
        rollup: sources.rollup.clone(),
        roster: Roster {
            entries: take_within(&sources.roster.entries, budget.roster),
        },
        episodes: take_within(&sources.episodes, budget.episodes),
        recalled,
        inbox: sources.inbox.clone(),
    };
    let keep = budget.full_steps.0 as usize;
    while task_fit(budget, &view) == TaskFit::Overflows && view.history.len() > keep {
        view.history.remove(0);
    }
    view
}
