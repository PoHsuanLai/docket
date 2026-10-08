//! `SessionEntry`: one thing that happened to a session, the unit of its durable log. The log is
//! append-only and ordered; folding it (`resume_plan`) is the only way to know where the session
//! stands. Entries hold the person's words, the checked policy, typed steps, and handle *labels*:
//! never a handle's content, never a secret.
//!
//! Named apart from `docket_router::SessionRecord` (the router's in-memory session) and
//! `companion_wire::SessionRecord` (companiond's roster facts, read back by `legacy`).

use docket_core::{
    ActionRef, CallId, Handle, HandleShape, SkillId, SkillVersion, StepLine, TaskPolicy, UserTurn,
};
use docket_core::{BreakerTrip, Ledger};
use porter_core::AppName;
use prov::{AgentRef, Effect, Label, SessionId, Source, SpaceId, TaskId};
use serde::{Deserialize, Serialize};

/// Whether the session read anything untrusted. `Clean < Tainted`; taint never goes down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Taint {
    /// Nothing untrusted was revealed.
    Clean,
    /// Something was, and it stays so.
    Tainted,
}

impl Taint {
    /// The higher of two: the only way taint combines.
    pub fn join(self, other: Taint) -> Taint {
        self.max(other)
    }
}

// The name lives in docket-core, where a standing grant also names the program.
pub use docket_core::{ProgramName, ProgramNameError};

// The workspace lives in docket-core, where `Session.Open` carries it.
pub use docket_core::{Workspace, WorkspaceError};

/// What runs the session's turns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BackendKind {
    /// The planner over inferd (`docket-tasks`).
    Native,
    /// An external agent program behind the ACP client edge.
    Acp(ProgramName),
    /// The test backend.
    Fake,
}

/// Where a fork was cut: the parent and the last entry it kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForkPoint {
    /// The session forked from.
    pub session: SessionId,
    /// The last entry of it the fork holds.
    pub at: Seq,
}

/// A position in one session's log: the writer's own count from 0, carried in the stored body so
/// a lost entry shows as a gap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Seq(pub u64);

impl Seq {
    /// The next position.
    pub fn next(self) -> Seq {
        Seq(self.0.saturating_add(1))
    }
}

/// How a session began.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Opening {
    /// The task it runs.
    pub task: TaskId,
    /// Its Space.
    pub space: SpaceId,
    /// The app that opened it; `None` only for a session migrated from a record that never
    /// said.
    pub opener: Option<AppName>,
    /// The agent it is for, as the roster names it (`None` as for `opener`).
    pub agent: Option<AgentRef>,
    /// What runs its turns.
    pub backend: BackendKind,
    /// The task that spawned it.
    pub parent: Option<TaskId>,
    /// Set when it is a fork.
    pub forked_from: Option<ForkPoint>,
    /// The directory an editor opened it in; none for a session no editor opened.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<Workspace>,
}

/// A call the router accepted, written before its end: a `Call` with no `Step` after it is a call
/// the crash cut off.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CallOpen {
    /// The call.
    pub call: CallId,
    /// The action.
    pub action: ActionRef,
    /// Its effect.
    pub effect: Effect,
}

/// A handle as the log keeps it: what the planner was told, never what it holds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleLabel {
    /// The handle.
    pub handle: Handle,
    /// The router's label for the value.
    pub label: Label,
    /// What it holds, in shape only.
    pub shape: HandleShape,
    /// Where it came from.
    pub from: Source,
}

/// Why a session became tainted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaintCause {
    /// A plain untrusted value was about to reach a model.
    UntrustedReveal,
    /// A fork took its parent's taint.
    Inherited,
    /// A resume found untrusted values with no taint before them and wrote the one that was due.
    Repaired,
}

/// Taint, written before the value that causes it is released (write-ahead): an append that
/// fails means the reveal is refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaintNote {
    /// Why.
    pub cause: TaintCause,
    /// The call whose result is revealed, if one.
    pub at_call: Option<CallId>,
}

/// The breaker's two moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum BreakerNote {
    /// It tripped: the session waits for the person.
    Tripped(BreakerTrip),
    /// It was reset without a turn.
    Reset,
}

/// A skill the planner loaded: which and at what version, never the body.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillUse {
    /// The skill.
    pub id: SkillId,
    /// Its version.
    pub version: SkillVersion,
}

/// Why a session closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndCause {
    /// The caller closed it.
    Closed,
    /// Its Space was halted.
    SpaceHalted,
    /// The wall budget ran out.
    WallExhausted,
}

/// One thing that happened to a session. The kind tag in the eventlog is
/// `companion.session.<slug>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum SessionEntry {
    /// The session opened. First, once.
    Opened(Opening),
    /// The person said something, verbatim and trusted.
    Turn(UserTurn),
    /// The checked task policy as written; resume never derives it again.
    Policy(TaskPolicy),
    /// A call began.
    Call(CallOpen),
    /// A call ended, as the planner's history shows it.
    Step(StepLine),
    /// A handle the planner may name.
    Handle(HandleLabel),
    /// The session is tainted.
    Taint(TaintNote),
    /// The breaker moved.
    Breaker(BreakerNote),
    /// A checkpoint of what the session has used.
    Budget(Ledger),
    /// A skill loaded.
    Skill(SkillUse),
    /// The session closed.
    Closed(EndCause),
}

impl SessionEntry {
    /// The entry's slug, the tail of its kind tag.
    pub fn slug(&self) -> &'static str {
        match self {
            SessionEntry::Opened(_) => "opened",
            SessionEntry::Turn(_) => "turn",
            SessionEntry::Policy(_) => "policy",
            SessionEntry::Call(_) => "call",
            SessionEntry::Step(_) => "step",
            SessionEntry::Handle(_) => "handle",
            SessionEntry::Taint(_) => "taint",
            SessionEntry::Breaker(_) => "breaker",
            SessionEntry::Budget(_) => "budget",
            SessionEntry::Skill(_) => "skill",
            SessionEntry::Closed(_) => "closed",
        }
    }
}
