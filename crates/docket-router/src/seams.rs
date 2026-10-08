//! What the router is handed instead of reaching for it: the apps, the person's consent store,
//! the event log, the clock and memory. The seams that draw (`Confirmer`), review (`Reviewer`),
//! derive a policy (`PolicyWriter`) and read untrusted text (`Reader`) are traits of
//! `docket-core` and `action-review`; `Seams` bundles all of them so the router is generic over
//! one parameter and the implementations stay closed sets, not `dyn`.

use action_review::Reviewer;
use almanac_core::{MemoryReply, MemoryRequest};
use docket_core::EntityRef;
use docket_core::{
    ActionGrant, AppRefusal, AuditRecord, Confirmer, ContextScope, ContextSnapshot, Generation,
    Hit, Invocation, Latency, Millis, Outcome, PolicyWriter, Preview, Reader, Revocation,
    StandingGrant, StandingGrantId, SuggestAsk, UndoFault, UndoToken,
};
use docket_session::{Appended, LogFault, LogPage, PageSize, Seq, SessionEntry, SessionLog};
use porter_core::AppName;
use prov::{Actor, EntityId, SessionId, SpaceId, UnixSeconds};
use std::future::Future;

/// Why a call to an app or to memory got no answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, thiserror::Error)]
pub enum LinkFault {
    /// The app is not running and could not be started.
    #[error("unavailable")]
    Unavailable,
    /// It did not answer in time.
    #[error("timed out")]
    Timeout,
    /// It answered something that is not the protocol.
    #[error("malformed answer")]
    Malformed,
}

/// Why `Perform` got no outcome: the app said no, said nothing in time, or is not there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppFault {
    /// The app's own refusal.
    Refused(AppRefusal),
    /// It did not answer within the action's latency budget.
    TimedOut,
    /// It is not there: not running and not startable, or not the person's own process.
    Unavailable,
}

impl From<AppRefusal> for AppFault {
    fn from(refusal: AppRefusal) -> Self {
        AppFault::Refused(refusal)
    }
}

/// Calls `IntentProvider1` on an app's own bus name, after checking the name's owner derives to
/// the same `AppId`.
pub trait AppLink: Send + Sync {
    /// `Perform`, within the action's latency budget.
    fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> impl Future<Output = Result<Outcome, AppFault>> + Send;
    /// `Perform` with the launcher's activation token beside the invocation. A link that does
    /// not deliver tokens (the default) performs without it.
    fn perform_activated(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        within: Latency,
    ) -> impl Future<Output = Result<Outcome, AppFault>> + Send {
        let _ = activation;
        self.perform(app, inv, within)
    }
    /// `Perform` with the activation token and, for an action that classifies per call, the
    /// classification the router gated on. A link that does not deliver it (the default)
    /// performs without it.
    fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_core::CallClass>,
        within: Latency,
    ) -> impl Future<Output = Result<Outcome, AppFault>> + Send {
        let _ = classified;
        self.perform_activated(app, inv, activation, within)
    }
    /// `Classify`: what this call does, for an action that opts in to per-call effects. A link
    /// without it (the default) classifies nothing, and the declared effect holds.
    fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<docket_core::CallClass, docket_core::ClassifyFault>> + Send
    {
        let _ = (app, inv);
        async { Err(docket_core::ClassifyFault::Unsupported) }
    }
    /// `DryRun`: the concrete change, before it is made.
    fn dry_run(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<Preview, AppRefusal>> + Send;
    /// `Undo`.
    fn undo(
        &self,
        app: &AppName,
        token: &UndoToken,
        actor: &Actor,
    ) -> impl Future<Output = Result<(), UndoFault>> + Send;
    /// `Context`.
    fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> impl Future<Output = Result<ContextSnapshot, LinkFault>> + Send;
    /// `Search`, for kinds the app does not index.
    fn search(
        &self,
        app: &AppName,
        text: &str,
        generation: Generation,
    ) -> impl Future<Output = Result<Vec<Hit>, LinkFault>> + Send;
    /// `Preview`.
    fn preview(
        &self,
        app: &AppName,
        id: &EntityId,
    ) -> impl Future<Output = Result<Preview, LinkFault>> + Send;
    /// `Suggest`.
    fn suggest(
        &self,
        app: &AppName,
        ask: SuggestAsk,
    ) -> impl Future<Output = Result<Vec<EntityRef>, LinkFault>> + Send;
}

/// The person's standing consent.
pub trait GrantStore: Send + Sync {
    /// Every grant.
    fn grants(&self) -> Vec<ActionGrant>;
    /// Records a grant the person gave.
    fn record(&self, grant: ActionGrant);
    /// Every standing grant ("allow always", scoped). A store that keeps none holds none, so
    /// every such call asks.
    fn standing(&self) -> Vec<StandingGrant> {
        Vec::new()
    }
    /// Holds a standing grant the person gave; the same id replaces the earlier one.
    fn add_standing(&self, _grant: StandingGrant) {}
    /// Drops a standing grant; it takes effect on the next call.
    fn revoke_standing(&self, _id: &StandingGrantId) -> Revocation {
        Revocation::NotHeld
    }
}

/// The event log, as the router sees it.
pub trait EventSink: Send + Sync {
    /// Appends one record: never content.
    fn append(&self, record: AuditRecord);
}

/// The one clock.
pub trait Clock: Send + Sync {
    /// Now.
    fn now(&self) -> UnixSeconds;
    /// Completes after `wait`: the deadline the router races a reviewer against (the real clock
    /// sleeps; a fake completes at once only for no time at all).
    fn after(&self, wait: Millis) -> impl Future<Output = ()> + Send;
}

/// Memory, reached as `Caller::Router` (the router acts for the companion's session, in the
/// Space of the invocation; the companion never calls memory itself).
pub trait MemoryLink: Send + Sync {
    /// One memory request, answered by memoryd.
    fn ask(
        &self,
        request: MemoryRequest,
    ) -> impl Future<Output = Result<MemoryReply, LinkFault>> + Send;

    /// Deletes what memory keeps for a Space that was removed, rather than moving it to any other
    /// Space. A link without that operation says [`SpaceMemories::Kept`], which leaves the
    /// records where they are, unreachable from a Space that is gone.
    fn erase_space(&self, _space: &SpaceId) -> impl Future<Output = SpaceMemories> + Send {
        async { SpaceMemories::Kept }
    }
}

/// What became of a removed Space's memories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpaceMemories {
    /// Memory was left as it was.
    Kept,
    /// Memory deleted them.
    Deleted,
}

/// The seam to a session's durable record when there is none: every entry is accepted and
/// nothing is kept, so a session cannot be restored. What an app that hosts its own agent
/// without memory runs with; the daemon passes `docket-memory`'s log over almanac.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoLog;

impl SessionLog for NoLog {
    async fn append(
        &self,
        _session: &SessionId,
        seq: Seq,
        _entry: &SessionEntry,
    ) -> Result<Appended, LogFault> {
        Ok(Appended { seq })
    }

    async fn page(
        &self,
        _session: &SessionId,
        _from: Option<Seq>,
        _size: PageSize,
    ) -> Result<LogPage, LogFault> {
        Ok(LogPage {
            rows: Vec::new(),
            next: None,
        })
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        Ok(Vec::new())
    }
}

/// The seams of one router, bundled. A closed set per seam: the real one, and the fake a test
/// drives.
pub trait Seams: Send + Sync {
    /// The apps.
    type Link: AppLink;
    /// The sheet that asks the person.
    type Confirm: Confirmer;
    /// The reviewer cascade.
    type Review: Reviewer;
    /// The consent store.
    type Grants: GrantStore;
    /// The event log.
    type Sink: EventSink;
    /// The clock.
    type Time: Clock;
    /// Memory.
    type Memory: MemoryLink;
    /// The task-policy writer.
    type Writer: PolicyWriter;
    /// The quarantined reader.
    type Reading: Reader;
    /// The durable record of sessions.
    type Log: SessionLog;

    /// The apps.
    fn link(&self) -> &Self::Link;
    /// The sheet.
    fn confirmer(&self) -> &Self::Confirm;
    /// The reviewer.
    fn reviewer(&self) -> &Self::Review;
    /// The consent store.
    fn grants(&self) -> &Self::Grants;
    /// The event log.
    fn sink(&self) -> &Self::Sink;
    /// The clock.
    fn clock(&self) -> &Self::Time;
    /// Memory.
    fn memory(&self) -> &Self::Memory;
    /// The policy writer.
    fn writer(&self) -> &Self::Writer;
    /// The reader.
    fn reader(&self) -> &Self::Reading;
    /// The session log.
    fn log(&self) -> &Self::Log;
}
