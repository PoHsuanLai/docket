//! Tasks: the companion is one identity over many tasks, and each task is its own docket
//! `Session` (its own taint, budget and `TaskPolicy`). This is the vocabulary of opening one,
//! spawning a worker, and the ledger the router keeps so a finished task becomes an episode.

use crate::call::{ArgFault, CallEnd};
use crate::ids::ParamName;
use crate::ids::{CallId, UndoId};
use crate::planner::UserTurn;
use crate::value::{Args, Value};
use almanac_core::{
    Episode, EpisodeId, EpisodeKind, EpisodeOutcome, ResultLine, Skeleton, StepLine, StepOutcome,
    ThingRole, ThingView,
};
use prov::{
    ActionName, AgentRef, Confidentiality, Effect, EntityId, Integrity, Label, MessageText,
    SessionId, Source, SpaceId, TaskId, UndoHandle, UnixSeconds,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// What a worker task is for. Workers beyond computer-use runs are frozen as types; computer
/// use has its own action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskKind {
    /// Look something up and report.
    Research,
    /// Watch for something and report when it happens.
    Watch,
    /// Do a job in the background.
    Background,
}

/// The arguments of the `companion.task.start` action, as parsed from its `Args`. The goal is
/// the planner's own text and never feeds a sink.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskStart {
    /// What the worker should do.
    pub goal: MessageText,
    /// What kind of worker.
    pub kind: TaskKind,
}

impl TaskStart {
    /// Reads the arguments of `companion.task.start`: `goal` is text and `kind` one of the
    /// declared choices (research when it is left out, as the manifest defaults it).
    pub fn from_args(args: &Args) -> Result<Self, ArgFault> {
        let name = |n: &str| ParamName::parse(n).map_err(|_| ArgFault::Missing);
        let goal = match args.get(&name("goal")?).map(|a| &a.value) {
            Some(Value::Text(t)) if !t.is_empty() => MessageText::new(t.clone()),
            Some(Value::Text(_)) | None => return Err(ArgFault::Missing),
            Some(_) => return Err(ArgFault::WrongType),
        };
        let kind = match args.get(&name("kind")?).map(|a| &a.value) {
            None => TaskKind::Research,
            Some(Value::Choice(id)) => match id.as_str() {
                "research" => TaskKind::Research,
                "watch" => TaskKind::Watch,
                "background" => TaskKind::Background,
                _ => return Err(ArgFault::OutOfRange),
            },
            Some(_) => return Err(ArgFault::WrongType),
        };
        Ok(Self { goal, kind })
    }
}

/// Opens a session: a front conversation, a worker, a computer-use run or a side conversation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOpen {
    /// The Space it works in.
    pub space: SpaceId,
    /// Which agent it is for.
    pub agent: AgentRef,
    /// The task that spawned it; the child policy is never wider than the parent's.
    pub parent: Option<TaskId>,
    /// The directory an editor opened it in; none for a session no editor opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<crate::workspace::Workspace>,
}

/// The session the router opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionOpened {
    /// The session.
    pub session: SessionId,
    /// The task it runs.
    pub task: TaskId,
}

/// One step of a task, as the router remembers it for the episode.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LedgerStep {
    /// The call.
    pub call: CallId,
    /// The action.
    pub action: ActionName,
    /// What it touched.
    pub targets: Vec<EntityId>,
    /// Its effect.
    pub effect: Effect,
    /// How it ended.
    pub end: CallEnd,
    /// The journal row, if undoable.
    pub undo: Option<UndoId>,
}

/// Everything the router keeps of a task until it ends. Trusted by construction: the person's
/// own turns, typed steps, entity ids and closed-set results; never content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaskLedger {
    /// The task.
    pub task: TaskId,
    /// The agent that ran it.
    pub agent: AgentRef,
    /// The episode of the task that spawned it.
    pub parent: Option<EpisodeId>,
    /// Its Space.
    pub space: SpaceId,
    /// When it started.
    pub started: UnixSeconds,
    /// The person's turns.
    pub asked: Vec<UserTurn>,
    /// Its calls.
    pub steps: Vec<LedgerStep>,
    /// Things it touched, for cascade-forget.
    pub touched: Vec<(ThingView, ThingRole)>,
    /// Typed results.
    pub results: Vec<ResultLine>,
}

/// How a call's end reads in an episode.
pub fn step_outcome(end: &CallEnd) -> StepOutcome {
    use crate::call::CallRefusal as R;
    match end {
        CallEnd::Done => StepOutcome::Done,
        CallEnd::Refused(R::Denied(_) | R::Unconfirmed(_) | R::Paused(_) | R::Halted(_)) => {
            StepOutcome::Denied
        }
        CallEnd::Refused(R::NoSuchAction(_) | R::BadArgs { .. }) => StepOutcome::Skipped,
        CallEnd::Refused(
            R::App(_) | R::OverBudget(_) | R::AppUnavailable(_) | R::Timeout | R::NotRecorded,
        ) => StepOutcome::Failed,
    }
}

/// The skeleton of a task: trusted, built without a model. Its label is `Trusted` and private
/// to the task's Space; text enters it only as the person's own words.
pub fn skeleton_of(ledger: &TaskLedger) -> Skeleton {
    Skeleton {
        label: Label {
            integrity: Integrity::Trusted,
            confidentiality: Confidentiality::Private(BTreeSet::from([ledger.space.clone()])),
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::User]),
        },
        asked: ledger
            .asked
            .iter()
            .map(|t| MessageText::new(t.text.clone()))
            .collect(),
        steps: ledger
            .steps
            .iter()
            .map(|s| StepLine {
                action: s.action.clone(),
                targets: s.targets.clone(),
                effect: s.effect,
                outcome: step_outcome(&s.end),
                undo: s
                    .undo
                    .and_then(|u| UndoHandle::parse(&u.0.to_string()).ok()),
            })
            .collect(),
        touched: ledger.touched.clone(),
        results: ledger.results.clone(),
    }
}

/// The episode a task leaves when it ends: the deterministic skeleton and no narrative (the
/// idle pass writes a second event with the narrative). `None` when the task id is not a valid
/// episode id.
pub fn close(
    ledger: &TaskLedger,
    kind: EpisodeKind,
    ended: UnixSeconds,
    outcome: EpisodeOutcome,
) -> Option<Episode> {
    Some(Episode {
        id: EpisodeId::parse(ledger.task.as_str()).ok()?,
        agent: ledger.agent.clone(),
        kind,
        parent: ledger.parent.clone(),
        space: ledger.space.clone(),
        started: ledger.started,
        ended,
        outcome,
        skeleton: skeleton_of(ledger),
        narrative: None,
    })
}
