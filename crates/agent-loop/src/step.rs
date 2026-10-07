//! The planner loop: one task's turn-by-turn machine. companiond feeds it events and carries
//! out the effects. The planner is a model that calls typed actions; every call goes through
//! the router, which gates it, so nothing here decides what is allowed.

use crate::completion::{Attention, CompletionNote, completion_line};
use crate::guard::{Guard, Verdict};
use crate::tier::Tier;
use companion_wire::{AnswerPhase, NeedsYou};
use docket_core::{
    BreakerTrip, CallId, CallRefusal, CallRequest, Held, ReaderAsk, ReplyFault, Reveal, StepEnd,
    TurnId, Value,
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
    /// What this turn has called and what it came to: it holds back a call that repeats one that
    /// got nothing.
    #[serde(default)]
    pub guard: Guard,
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
    /// A reply that could not be read as a call: told to the planner on its next turn, and
    /// counted, so a model that cannot get it right ends in a question to the person.
    Unread(ReplyFault),
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
    /// The read gave no answer the planner can use, and it may write the read again.
    ReadFailed(ReplyFault),
    /// The model failed or timed out.
    ModelFailed,
    /// A worker or run reported.
    Completed(CompletionNote),
    /// A request landed in the task's inbox while it was waiting for something to do (the goal
    /// of a worker, a message from another Space). It is input like the person's turn, with no
    /// words of the person's: the planner reads it from the view's inbox, and every call it
    /// then plans is gated by the task's own policy.
    Messaged,
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
    /// The call was not made, because the planner had made it before and nothing had changed:
    /// the history says so, and the planner is asked again.
    Held(Box<CallRequest>, Held),
    /// The planner's reply could not be read: add a line to the task's history saying why, and
    /// ask the planner again.
    Unread(ReplyFault),
    /// The call was refused: tell the planner only the coarse code, and count the retry.
    Refused(CallRefusal),
    /// Write the task's episode.
    CloseTask,
}

/// One transition. The router mints call ids, after the loop has asked for the call, so a
/// batch's calls are named by their position in it (`CallId(0)`, `CallId(1)`, ...): the
/// `Call` effects come out in that order, a batch is only issued with nothing pending, and
/// companiond reports each end as `CallEnded` with the position. `Planned` is one output of the planner's reply; a reply is a run of `Say`s
/// ending in `Calls`, `Read`, `Ask` or `Finish` (companiond sends `Finish` for a reply that
/// only spoke). Inputs that do not fit the phase change nothing, so a late or duplicate event
/// is harmless.
pub fn agent_step(state: LoopState, input: LoopInput) -> (LoopState, Vec<LoopEffect>) {
    use LoopPhase::{AwaitingCalls, AwaitingReader, Finished, Idle, Paused, Planning};
    match (state.phase, input) {
        (Finished(_), _) => stay(state),
        (_, LoopInput::Cancelled | LoopInput::Halted) => finish(state, FinishedAs::Cancelled),
        (_, LoopInput::Completed(note)) => (state, completion_effects(&note)),
        (Idle, LoopInput::Messaged) => ask_for_message(state),
        (Idle, LoopInput::Asked(turn)) => ask_planner(state, turn),
        (Planning, LoopInput::Asked(turn)) => ask_planner(state, turn),
        (_, LoopInput::Asked(turn)) => stay(LoopState {
            turn: Some(turn),
            ..state
        }),
        (Planning, LoopInput::Planned(output)) => planned(state, output),
        (Planning | AwaitingReader, LoopInput::ModelFailed) => finish(state, FinishedAs::Failed),
        (AwaitingReader, LoopInput::ReadAnswered(_)) => replan(state),
        (AwaitingReader, LoopInput::ReadFailed(fault)) => {
            unreadable(with_phase(state, Planning), fault)
        }
        (AwaitingCalls | Paused(_), LoopInput::CallEnded(id, end)) => call_ended(state, id, end),
        (Paused(_), LoopInput::Resumed) => resumed(state),
        (Paused(_), LoopInput::Tripped(_)) => stay(state),
        (Idle | Planning | AwaitingCalls | AwaitingReader, LoopInput::Tripped(trip)) => {
            pause(state, trip)
        }
        _ => stay(state),
    }
}

fn stay(state: LoopState) -> (LoopState, Vec<LoopEffect>) {
    (state, vec![])
}

fn with_phase(state: LoopState, phase: LoopPhase) -> LoopState {
    LoopState { phase, ..state }
}

fn ask_planner(state: LoopState, turn: TurnId) -> (LoopState, Vec<LoopEffect>) {
    let next = LoopState {
        phase: LoopPhase::Planning,
        turn: Some(turn),
        pending: vec![],
        guard: Guard::default(),
        ..state
    };
    (
        next,
        vec![
            LoopEffect::Publish(AnswerPhase::Thinking),
            LoopEffect::AskPlanner,
        ],
    )
}

fn ask_for_message(state: LoopState) -> (LoopState, Vec<LoopEffect>) {
    let next = LoopState {
        phase: LoopPhase::Planning,
        pending: vec![],
        guard: Guard::default(),
        ..state
    };
    (
        next,
        vec![
            LoopEffect::Publish(AnswerPhase::Thinking),
            LoopEffect::AskPlanner,
        ],
    )
}

fn replan(state: LoopState) -> (LoopState, Vec<LoopEffect>) {
    (
        with_phase(state, LoopPhase::Planning),
        vec![LoopEffect::AskPlanner],
    )
}

fn planned(state: LoopState, output: ModelOutput) -> (LoopState, Vec<LoopEffect>) {
    let state = LoopState {
        steps: state.steps.saturating_add(1),
        ..state
    };
    match output {
        ModelOutput::Calls(calls) if calls.is_empty() => finish(state, FinishedAs::Failed),
        ModelOutput::Calls(calls) => admitted(state, calls),
        ModelOutput::Unread(fault) => unreadable(state, fault),
        ModelOutput::Read(ask) => (
            with_phase(state, LoopPhase::AwaitingReader),
            vec![LoopEffect::Read(Box::new(ask))],
        ),
        ModelOutput::Say(_) => (state, vec![LoopEffect::Publish(AnswerPhase::Streaming)]),
        ModelOutput::Ask { text, choices } => (
            with_phase(state, LoopPhase::Idle),
            vec![LoopEffect::Publish(AnswerPhase::NeedsYou(
                NeedsYou::Question { text, choices },
            ))],
        ),
        ModelOutput::Finish => finish(state, FinishedAs::Done),
    }
}

/// A reply that could not be read: the planner is told so and asked again, until it has done so
/// too often running and the person is asked instead.
fn unreadable(state: LoopState, fault: ReplyFault) -> (LoopState, Vec<LoopEffect>) {
    let (guard, stuck) = state.guard.clone().unreadable();
    let state = LoopState { guard, ..state };
    match stuck {
        None => (
            state,
            vec![LoopEffect::Unread(fault), LoopEffect::AskPlanner],
        ),
        Some(stuck) => stopped(state, vec![LoopEffect::Unread(fault)], &stuck.question()),
    }
}

/// A batch of calls through the guard: those it lets by go out, those it holds back are told to
/// the planner, and a call that will not change course ends the turn with a question.
fn admitted(state: LoopState, calls: Vec<PlannedCall>) -> (LoopState, Vec<LoopEffect>) {
    let mut guard = state.guard.clone().settled();
    let mut run = Vec::new();
    let mut held = Vec::new();
    for planned in calls {
        let (next, verdict) = guard.admit(CallId(run.len() as u64), &planned.call);
        guard = next;
        match verdict {
            Verdict::Run => run.push(planned.call),
            Verdict::Hold(why) => held.push(LoopEffect::Held(Box::new(planned.call), why)),
            Verdict::Stop(stuck) => return stopped(state, held, &stuck.question()),
        }
    }
    let state = LoopState { guard, ..state };
    if run.is_empty() {
        let (next, effects) = replan(state);
        return (next, [held, effects].concat());
    }
    let pending = (0..run.len() as u64).map(CallId).collect();
    let calls = run.into_iter().map(|c| LoopEffect::Call(Box::new(c)));
    let next = LoopState {
        phase: LoopPhase::AwaitingCalls,
        pending,
        ..state
    };
    (next, held.into_iter().chain(calls).collect())
}

/// The turn comes to rest asking the person, never done with nothing to show.
fn stopped(state: LoopState, held: Vec<LoopEffect>, text: &str) -> (LoopState, Vec<LoopEffect>) {
    let question = AnswerPhase::NeedsYou(NeedsYou::Question {
        text: text.to_owned(),
        choices: vec![],
    });
    let next = LoopState {
        phase: LoopPhase::Idle,
        pending: vec![],
        ..state
    };
    (next, [held, vec![LoopEffect::Publish(question)]].concat())
}

fn call_ended(state: LoopState, id: CallId, end: StepEnd) -> (LoopState, Vec<LoopEffect>) {
    if !state.pending.contains(&id) {
        return stay(state);
    }
    let state = LoopState {
        guard: state.guard.clone().ended(id, &end),
        ..state
    };
    let pending: Vec<CallId> = state.pending.iter().copied().filter(|p| *p != id).collect();
    let state = LoopState { pending, ..state };
    let refusal = match end {
        StepEnd::Refused(refusal) => Some(refusal),
        StepEnd::Done { .. }
        | StepEnd::Unconfirmed(_)
        | StepEnd::Held(_)
        | StepEnd::Unread(_)
        | StepEnd::Interrupted => None,
    };
    let told: Vec<LoopEffect> = refusal.iter().cloned().map(LoopEffect::Refused).collect();
    match refusal {
        Some(CallRefusal::Halted(_)) => finish(state, FinishedAs::Cancelled),
        Some(CallRefusal::OverBudget(_)) => finish_after(state, FinishedAs::Failed, told),
        Some(CallRefusal::Paused(trip)) => {
            let (next, mut effects) = pause(state, trip);
            effects.splice(0..0, told);
            (next, effects)
        }
        _ if matches!(state.phase, LoopPhase::Paused(_)) || !state.pending.is_empty() => {
            (state, told)
        }
        _ => {
            let (next, effects) = replan(state);
            (next, [told, effects].concat())
        }
    }
}

fn resumed(state: LoopState) -> (LoopState, Vec<LoopEffect>) {
    if state.pending.is_empty() {
        replan(state)
    } else {
        (with_phase(state, LoopPhase::AwaitingCalls), vec![])
    }
}

fn pause(state: LoopState, trip: BreakerTrip) -> (LoopState, Vec<LoopEffect>) {
    let text = format!("Paused after repeated refusals ({trip:?}); say something to go on.");
    (
        with_phase(state, LoopPhase::Paused(trip)),
        vec![LoopEffect::Publish(AnswerPhase::NeedsYou(
            NeedsYou::Question {
                text,
                choices: vec![],
            },
        ))],
    )
}

fn finish(state: LoopState, how: FinishedAs) -> (LoopState, Vec<LoopEffect>) {
    finish_after(state, how, vec![])
}

fn finish_after(
    state: LoopState,
    how: FinishedAs,
    before: Vec<LoopEffect>,
) -> (LoopState, Vec<LoopEffect>) {
    let shown = match how {
        FinishedAs::Done => AnswerPhase::Done,
        FinishedAs::Failed => AnswerPhase::Failed,
        FinishedAs::Cancelled => AnswerPhase::Cancelled,
    };
    let next = LoopState {
        phase: LoopPhase::Finished(how),
        pending: vec![],
        ..state
    };
    let effects = [
        before,
        vec![LoopEffect::Publish(shown), LoopEffect::CloseTask],
    ]
    .concat();
    (next, effects)
}

/// A completion note is a line in the task, and the answer waits for the person only when the
/// result needs them. It never starts anything.
fn completion_effects(note: &CompletionNote) -> Vec<LoopEffect> {
    let Some(line) = completion_line(note) else {
        return vec![];
    };
    let waits = match note.attention {
        Attention::NeedsYou => Some(LoopEffect::Publish(AnswerPhase::NeedsYou(
            NeedsYou::Question {
                text: line.clone(),
                choices: vec![],
            },
        ))),
        Attention::Quiet => None,
    };
    std::iter::once(LoopEffect::Note(line))
        .chain(waits)
        .collect()
}
