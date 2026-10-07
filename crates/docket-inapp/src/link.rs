//! The router's `AppLink` over the app's own provider: there is one app, so there is no name to
//! find and no owner to check. A call for any other app is unavailable.

use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    AppRefusal, ClassifyFault, ContextScope, ContextSnapshot, EntityRef, Generation, Hit,
    Invocation, Latency, Outcome, Preview, SuggestAsk, UndoFault, UndoToken,
};
use docket_router::{AppFault, AppLink, LinkFault};
use porter_core::AppName;
use prov::{Actor, EntityId};

/// One provider and the source of its window context, as the router's apps.
#[derive(Debug)]
pub struct ProviderLink<P, C> {
    provider: P,
    context: C,
}

impl<P: IntentProvider, C: ContextSource> ProviderLink<P, C> {
    /// Links the router to `provider`, with `context` answering `Context`.
    pub fn new(provider: P, context: C) -> Self {
        Self { provider, context }
    }

    /// The provider, for the app to read what it holds.
    pub fn provider(&self) -> &P {
        &self.provider
    }

    fn is_ours(&self, app: &AppName) -> bool {
        self.provider.manifest().manifest().app == *app
    }
}

impl<P: IntentProvider, C: ContextSource> AppLink for ProviderLink<P, C> {
    async fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        self.perform_classified(app, inv, None, None, within).await
    }

    async fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_core::CallClass>,
        _within: Latency,
    ) -> Result<Outcome, AppFault> {
        if !self.is_ours(app) {
            return Err(AppFault::Unavailable);
        }
        self.provider
            .perform_classified(inv, activation, classified)
            .await
            .map_err(AppFault::Refused)
    }

    async fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> Result<docket_core::CallClass, ClassifyFault> {
        if !self.is_ours(app) {
            return Err(ClassifyFault::Unsupported);
        }
        self.provider
            .classify(inv)
            .await
            .map_err(|_| ClassifyFault::Refused)
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        if !self.is_ours(app) {
            return Err(AppRefusal::Unsupported);
        }
        self.provider.dry_run(inv).await
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        if !self.is_ours(app) {
            return Err(UndoFault::AppUnavailable);
        }
        self.provider.undo(token.clone(), actor.clone()).await
    }

    async fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        if !self.is_ours(app) {
            return Err(LinkFault::Unavailable);
        }
        Ok(self.context.snapshot(scope))
    }

    async fn search(
        &self,
        app: &AppName,
        text: &str,
        _generation: Generation,
    ) -> Result<Vec<Hit>, LinkFault> {
        if !self.is_ours(app) {
            return Err(LinkFault::Unavailable);
        }
        Ok(self.provider.search(text).await)
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        if !self.is_ours(app) {
            return Err(LinkFault::Unavailable);
        }
        Ok(self.provider.preview(id).await)
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        if !self.is_ours(app) {
            return Err(LinkFault::Unavailable);
        }
        Ok(self.provider.suggest(ask).await)
    }
}
