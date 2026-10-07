//! The companion's pure machines. One identity over many tasks: each task is its own docket
//! session with its own taint, budget and task policy; the front thread is the task the
//! launcher returns to. Everything here is `(state, input) -> (state, effects)` or a function of
//! values; companiond carries the effects out. Inputs, states and effects are `Serialize` and
//! `Eq`, so a run can be traced and replayed.
//!
//! - `assemble`: the working-set assembler, sections from most to least stable.
//! - `agent_step`: the planner loop.
//! - `Guard`: the turn's ledger of calls; it holds back a repeated call that got nothing and
//!   stops a turn that will not change course.
//! - `choose_tier`: typed action, then hook, then computer use.
//! - `side_step`: capture of the person's side conversations with a subagent.
//! - `idle_step`: the background narrative pass, preempted by anything interactive.
//! - `leaked_call`: whether a model's words are a tool call that was never made.
//! - `completion_line`: the one-line note a finished worker or run leaves in the front task.
//! - `rebuild`: the roster and the front task after a restart, from the stored records.
//! - `front_step`: where the launcher returns to.

mod assemble;
mod completion;
mod front;
mod guard;
mod idle;
mod leak;
mod rebuild;
mod side;
mod step;
mod tier;

pub use assemble::{Sources, TaskFit, assemble, mask_history, task_fit};
pub use completion::{AskedPerson, Attention, CompletionNote, attention_of, completion_line};
pub use front::{FrontEvent, front_step};
pub use guard::{Guard, HOLDS_BEFORE_STOP, MOST_HOLDS_PER_TURN, MOST_REMEMBERED, Stuck, Verdict};
pub use idle::{
    EpisodeJob, IdleEffect, IdleInput, IdlePhase, IdleState, NarrativeJob, NarrativeVia,
    ReadUntrusted, idle_step,
};
pub use leak::{LeakForm, LeakedCall, leaked_call};
pub use rebuild::{Rebuilt, RebuiltTask, ReplayEvent, ReplayWhat, rebuild};
pub use side::{SideConv, SideEffect, SideEnd, SideInput, SideTable, side_episode, side_step};
pub use step::{
    FinishedAs, LoopEffect, LoopInput, LoopPhase, LoopState, ModelOutput, PlannedCall, agent_step,
};
pub use tier::{Availability, Offer, Tier, choose_tier};
