//! `SessionBackend` and `SessionHost`: the shapes of section 4 of the design note, as traits and
//! nothing else. A backend returns events and holds no authority: effects go through the
//! router's gate by a handle the host gives it (a later lane). The host writes the entries.
//! Events are pulled (`next_event`), so no stream type, executor or runtime is named here.

use crate::entry::{BackendKind, CallOpen, EndCause, Opening, Seq};
use crate::export::SessionExport;
use crate::plan::ResumePlan;
use companion_wire::NeedsYou;
use docket_core::{BreakerTrip, Reveal, StepLine, UserTurn};
use porter_core::{Count, MicroUsd};
use prov::SessionId;
use std::future::Future;

/// Why a backend could not do what it was asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BackendFault {
    /// Not started, or already closed.
    #[error("the backend is not running a session")]
    NotRunning,
    /// A turn is still being answered.
    #[error("a turn is already in progress")]
    Busy,
    /// The process or the model did not answer.
    #[error("the backend is unavailable")]
    Unavailable,
}

/// How a resume went.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resumed {
    /// The backend picked up its own session.
    Restored,
    /// It had lost it and began a new one from the plan; the person is told.
    Reseeded,
}

/// What a backend is told to begin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartSession {
    /// The session.
    pub session: SessionId,
    /// How it began.
    pub opening: Opening,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnd {
    /// The answer is complete.
    Done,
    /// It could not be completed.
    Failed,
    /// The backend declined.
    Refused,
    /// The person or the host cancelled it.
    Cancelled,
    /// The breaker tripped: the session waits for the person.
    Paused(BreakerTrip),
}

/// What a backend reports of its own use: informational, never trusted for a budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsageNote {
    /// The context it holds, in tokens, if it says.
    pub context: Option<Count>,
    /// What it reports spending, if it says.
    pub spent: Option<MicroUsd>,
}

/// A call's progress as the backend reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CallEvent {
    /// A call began.
    Started(CallOpen),
    /// It ended, as the router ended it.
    Ended(StepLine),
}

/// What a turn yields, in order; the last is always `TurnEnd`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendEvent {
    /// Words; handles stay handles.
    Words(Reveal<String>),
    /// Thinking, only when the person asked to see it.
    Thought(String),
    /// A call.
    Call(CallEvent),
    /// The person is needed.
    NeedsYou(NeedsYou),
    /// Use, informational.
    Usage(UsageNote),
    /// The turn is over.
    TurnEnd(TurnEnd),
}

/// What runs a session's turns: the planner, an external agent, a fake.
pub trait SessionBackend: Send {
    /// Which kind it is.
    fn kind(&self) -> BackendKind;

    /// Begins a new session.
    fn start(
        &mut self,
        open: StartSession,
    ) -> impl Future<Output = Result<(), BackendFault>> + Send;

    /// Takes a session back from its plan.
    fn resume(
        &mut self,
        plan: &ResumePlan,
    ) -> impl Future<Output = Result<Resumed, BackendFault>> + Send;

    /// Gives it the person's turn; its events follow from `next_event`.
    fn turn(&mut self, turn: UserTurn) -> impl Future<Output = Result<(), BackendFault>> + Send;

    /// The next event of the turn in progress; `None` when there is none.
    fn next_event(&mut self) -> impl Future<Output = Option<BackendEvent>> + Send;

    /// Asks it to stop; it answers `TurnEnd::Cancelled` or the host ends it.
    fn cancel(&mut self) -> impl Future<Output = ()> + Send;

    /// Releases it.
    fn close(&mut self) -> impl Future<Output = ()> + Send;
}

/// Why the host could not do what it was asked.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum HostFault {
    /// No such session.
    #[error("no such session")]
    NoSuchSession,
    /// Its log has no plan.
    #[error("the session cannot be resumed: {0}")]
    Plan(#[from] crate::plan::PlanRefusal),
    /// Its standing takes no turn (closed, blocked).
    #[error("the session takes no turn")]
    NotOpen,
    /// The log refused an entry.
    #[error("the log failed: {0}")]
    Log(#[from] crate::log::LogFault),
    /// The backend failed.
    #[error("the backend failed: {0}")]
    Backend(#[from] BackendFault),
    /// The session could not be forked there.
    #[error("the session cannot be forked: {0}")]
    Fork(#[from] crate::fork::ForkFault),
    /// The session could not be exported.
    #[error("the session cannot be exported: {0}")]
    Export(#[from] crate::export::ExportFault),
}

/// The registry of sessions: it writes the log, owns the backends, and answers the edges (the
/// companion, `quire-do`, the ACP server). Every method that changes a session appends its
/// entry first and answers after the ack.
pub trait SessionHost: Send {
    /// Opens a session and returns its id.
    fn open(
        &mut self,
        opening: Opening,
    ) -> impl Future<Output = Result<SessionId, HostFault>> + Send;

    /// The plan a session would resume from, and the session resumed on its backend.
    fn resume(
        &mut self,
        session: &SessionId,
    ) -> impl Future<Output = Result<ResumePlan, HostFault>> + Send;

    /// The person's turn, recorded and handed to the backend.
    fn turn(
        &mut self,
        session: &SessionId,
        turn: UserTurn,
    ) -> impl Future<Output = Result<(), HostFault>> + Send;

    /// The next event of the session's turn in progress.
    fn next_event(
        &mut self,
        session: &SessionId,
    ) -> impl Future<Output = Result<Option<BackendEvent>, HostFault>> + Send;

    /// Cancels the turn in progress.
    fn cancel(&mut self, session: &SessionId)
    -> impl Future<Output = Result<(), HostFault>> + Send;

    /// Closes the session for `cause`.
    fn close(
        &mut self,
        session: &SessionId,
        cause: EndCause,
    ) -> impl Future<Output = Result<(), HostFault>> + Send;

    /// A new session from this one's entries up to `at`.
    fn fork(
        &mut self,
        session: &SessionId,
        at: Seq,
    ) -> impl Future<Output = Result<SessionId, HostFault>> + Send;

    /// The session as a document.
    fn export(
        &self,
        session: &SessionId,
    ) -> impl Future<Output = Result<SessionExport, HostFault>> + Send;
}
