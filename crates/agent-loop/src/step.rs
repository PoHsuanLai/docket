//! The planner loop: one task's turn-by-turn machine. companiond feeds it events and carries
//! out the effects. The planner is a model that calls typed actions; every call goes through
//! the router, which gates it, so nothing here decides what is allowed.

use crate::completion::CompletionNote;
use crate::tier::Tier;
use companion_wire::AnswerPhase;
use docket_core::{
    BreakerTrip, CallId, CallRefusal, CallRequest, ReaderAsk, Reveal, StepEnd, TurnId, Value,
};
use serde::{Deserialize, Serialize};

/// Where a task's loop is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum LoopPhase {
    /// Nothing asked.
    Idle,
    /// The planner is working on a view.
    Planning,
    /// Calls are out; waiting for them to end.
    AwaitingCalls,
    /// The reader is working.
    AwaitingReader,
    /// Paused until the person speaks.
    Paused(BreakerTrip),
    /// Over.
    Finished(FinishedAs),
}

/// How a loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FinishedAs {
    /// Done.
    Done,
    /// Failed.
    Failed,
    /// Cancelled.
    Cancelled,
}

/// One task's loop state.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoopState {
    /// Where it is.
    pub phase: LoopPhase,
    /// The turn it is answering.
    pub turn: Option<TurnId>,
    /// How many model steps it took.
    pub steps: u32,
    /// The calls still out.
    pub pending: Vec<CallId>,
}

/// One call the planner planned, and how it will be reached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedCall {
    /// The call, with provenance on every argument.
    pub call: CallRequest,
    /// How it is reached.
    pub tier: Tier,
}

/// What the planner model produced.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ModelOutput {
    /// Calls to make, at once.
    Calls(Vec<PlannedCall>),
    /// Ask the reader to read some handles.
    Read(ReaderAsk),
    /// Words for the person.
    Say(String),
    /// A question for the person.
    Ask {
        /// The question.
        text: String,
        /// The choices.
        choices: Vec<String>,
    },
    /// Nothing more to do.
    Finish,
}

/// What happens to a task's loop.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum LoopInput {
    /// The person asked (the turn is already recorded).
    Asked(TurnId),
    /// The planner answered.
    Planned(ModelOutput),
    /// A call ended.
    CallEnded(CallId, StepEnd),
    /// The reader answered.
    ReadAnswered(Reveal<Value>),
    /// The model failed or timed out.
    ModelFailed,
    /// A worker or run reported.
    Completed(CompletionNote),
    /// The person said something after a trip.
    Resumed,
    /// The breaker tripped.
    Tripped(BreakerTrip),
    /// The person stopped it.
    Cancelled,
    /// A halt arrived.
    Halted,
}

/// What companiond does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum LoopEffect {
    /// Assemble the view and ask the planner model.
    AskPlanner,
    /// Make this call through the router.
    Call(Box<CallRequest>),
    /// Ask the reader through the router.
    Read(Box<ReaderAsk>),
    /// Publish this phase to the answer object.
    Publish(AnswerPhase),
    /// Add a line to the task's history (a completion note).
    Note(String),
    /// The call was refused: tell the planner only the coarse code, and count the retry.
    Refused(CallRefusal),
    /// Write the task's episode.
    CloseTask,
}

/// One transition.
pub fn agent_step(state: LoopState, input: LoopInput) -> (LoopState, Vec<LoopEffect>) {
    let _ = (state, input);
    todo!(
        "agent_step: Idle -> Planning on Asked; Planning -> AwaitingCalls/AwaitingReader/Finished on the model's output; each CallEnded returns to Planning when none are pending; a refusal tells the planner the coarse code; Tripped pauses until Resumed; Halted and Cancelled finish; a Completed note is appended to the current task and never starts one"
    )
}
