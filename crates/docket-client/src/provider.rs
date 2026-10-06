//! What an app implements.

use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, EntityRef, Hit, Invocation, Outcome, Preview,
    SuggestAsk, SummonAnswer, SummonOrigin, SummonSerial, UndoFault, UndoToken, ValidManifest,
};
use prov::{Actor, EntityId};
use std::future::Future;

/// What an app implements to take part. `context` and `summon` have no method here: ds answers
/// them through [`ContextSource`] and [`SummonTarget`], which `docket-ds` implements.
pub trait IntentProvider: Send + Sync {
    /// The app's manifest.
    fn manifest(&self) -> &ValidManifest;
    /// Performs an action. The app labels its undo entry with `inv.actor`.
    fn perform(&self, inv: Invocation) -> impl Future<Output = Result<Outcome, AppRefusal>> + Send;
    /// Performs an action the launcher started, with its activation token (a capability to take
    /// focus, to hand to the compositor). The default ignores the token.
    fn perform_activated(
        &self,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
    ) -> impl Future<Output = Result<Outcome, AppRefusal>> + Send {
        let _ = activation;
        self.perform(inv)
    }
    /// Performs an action the router gated on `classified` (an action that classifies per
    /// call; `None` for every other). Re-derive the classification here: when it no longer
    /// matches, refuse with `AppRefusal::ClassificationChanged` and the router asks again at
    /// the declared ceiling. `classified` equal to the declared ceiling must always be
    /// accepted. The default ignores it.
    fn perform_classified(
        &self,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_core::CallClass>,
    ) -> impl Future<Output = Result<Outcome, AppRefusal>> + Send {
        let _ = classified;
        self.perform_activated(inv, activation)
    }
    /// What this call does, for an action that declares `per_call = "classified"`: an effect
    /// (never trusted past the declared one) or `Delegates` to another typed action. The
    /// default refuses, which leaves the declared effect in force.
    fn classify(
        &self,
        inv: Invocation,
    ) -> impl Future<Output = Result<docket_core::CallClass, AppRefusal>> + Send {
        let _ = inv;
        async { Err(AppRefusal::Unsupported) }
    }
    /// Describes the change without making it: what the confirmation shows.
    fn dry_run(&self, inv: Invocation) -> impl Future<Output = Result<Preview, AppRefusal>> + Send;
    /// Undoes one change.
    fn undo(
        &self,
        token: UndoToken,
        actor: Actor,
    ) -> impl Future<Output = Result<(), UndoFault>> + Send;
    /// Searches kinds the app does not index.
    fn search(&self, text: &str) -> impl Future<Output = Vec<Hit>> + Send;
    /// A preview of one thing.
    fn preview(&self, id: &EntityId) -> impl Future<Output = Preview> + Send;
    /// Options for a parameter.
    fn suggest(&self, ask: SuggestAsk) -> impl Future<Output = Vec<EntityRef>> + Send;
}

/// Reports what the person is looking at. `docket-ds` implements it over ds's context model.
pub trait ContextSource: Send + Sync {
    /// The context of the scope, taken now.
    fn snapshot(&self, scope: ContextScope) -> ContextSnapshot;
}

/// Takes a summon. `docket-ds` implements it over ds's companion port.
pub trait SummonTarget: Send + Sync {
    /// Answers a summon: the field became the prompt, an anchored prompt appeared, the previous
    /// prompt returned, or not here.
    fn summon(&self, serial: SummonSerial, origin: SummonOrigin) -> SummonAnswer;
}
