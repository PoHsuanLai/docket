//! What the router is handed instead of reaching for it: the apps, the person's consent store,
//! the event log, the clock and memory. The seams that draw (`Confirmer`), review (`Reviewer`),
//! derive a policy (`PolicyWriter`) and read untrusted text (`Reader`) are traits of
//! `docket-core` and `action-review`; `Seams` bundles all of them so the router is generic over
//! one parameter and the implementations stay closed sets, not `dyn`.

use action_review::Reviewer;
use docket_checkpoint::CheckpointStore;
use docket_core::Limit;
use docket_core::{
    ActionGrant, AppRefusal, Confirmer, ContextScope, ContextSnapshot, Invocation, Latency,
    Outcome, PolicyWriter, Preview, Reader, Revocation, StandingGrant, StandingGrantId, SuggestAsk,
    UndoFault,
};
use docket_core::{EventSink, LinkFault, MemoryLink};
use docket_session::{Appended, LogFault, LogPage, PageSize, Seq, SessionEntry, SessionLog};
use docket_vocab::{EntityRef, UndoToken};
use policy_point::Presets;
use porter_core::AppName;
use prov::{Actor, EntityId, SessionId};
use std::future::Future;

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
        classified: Option<docket_vocab::CallClass>,
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
    ) -> impl Future<Output = Result<docket_vocab::CallClass, docket_vocab::ClassifyFault>> + Send
    {
        let _ = (app, inv);
        async { Err(docket_vocab::ClassifyFault::Unsupported) }
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
    /// Whether the app is running now. A lookup (search, preview, context) asks only an app that
    /// is: typing in the launcher never starts an app. An action may start one.
    fn running(&self, app: &AppName) -> impl Future<Output = bool> + Send;
    /// `Context`.
    fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> impl Future<Output = Result<ContextSnapshot, LinkFault>> + Send;
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
    /// Drops a standing grant, or a limit with that id (the one revoke path); it takes effect
    /// on the next call.
    fn revoke_standing(&self, _id: &StandingGrantId) -> Revocation {
        Revocation::NotHeld
    }
    /// Every "always ask" and "never" entry. A store that keeps none holds none.
    fn limits(&self) -> Vec<Limit> {
        Vec::new()
    }
    /// Holds a limit; the same id replaces the earlier one. Removing one is `revoke_standing`.
    /// This is the older store, still read; a new limit is a rule (`add_rule`).
    fn add_limit(&self, _limit: Limit) {}
    /// Writes the forbid rule for `limit` into its rules file (the person's `never.cedar` or the
    /// assistant's `assistant.cedar`); the same id replaces the earlier rule, and the one revoke
    /// path removes it. A store that keeps no rules files writes none.
    fn add_rule(&self, _limit: &Limit) {}
    /// The policy sets the rules files make now, after a rule was added or removed; none when the
    /// store keeps no rules files or the files do not load (the sets in force then stay).
    fn rebuilt(&self) -> Option<Presets> {
        None
    }
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
    type Time: docket_vocab::Clock;
    /// Memory.
    type Memory: MemoryLink;
    /// The task-policy writer.
    type Writer: PolicyWriter;
    /// The quarantined reader.
    type Reading: Reader;
    /// The durable record of sessions.
    type Log: SessionLog;
    /// Where restore points of a session's workspace are kept (`NoStore` for a build that
    /// cannot keep history).
    type Checkpoints: CheckpointStore;

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
    /// The restore-point store.
    fn checkpoints(&self) -> &Self::Checkpoints;
}
