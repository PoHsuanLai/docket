//! The providers intentd answers in process, as a chain: each is asked for the app its manifest
//! names, so a new built-in is one more link in `HostedLink` and no new arm anywhere.

use docket_core::{AppRefusal, Invocation, Outcome, Preview, SuggestAsk, UndoFault};
use docket_manifest::ValidManifest;
use docket_provider::IntentProvider;
use docket_vocab::{EntityRef, UndoToken};
use porter_core::AppName;
use prov::{Actor, EntityId};
use std::future::Future;

/// What a chain of in-process providers answers. Every method but `serves` and `manifests` is
/// called only for an app `serves` accepted.
pub(crate) trait Builtins: Send + Sync {
    /// Whether one of the providers is the app `app`.
    fn serves(&self, app: &AppName) -> bool;
    /// The manifests of every provider, in order.
    fn manifests<'a>(&'a self, out: &mut Vec<&'a ValidManifest>);
    fn hosted_perform(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<Outcome, AppRefusal>> + Send;
    fn hosted_dry_run(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<Preview, AppRefusal>> + Send;
    fn hosted_undo(
        &self,
        app: &AppName,
        token: UndoToken,
        actor: Actor,
    ) -> impl Future<Output = Result<(), UndoFault>> + Send;
    fn hosted_preview(&self, app: &AppName, id: &EntityId) -> impl Future<Output = Preview> + Send;
    fn hosted_suggest(
        &self,
        app: &AppName,
        ask: SuggestAsk,
    ) -> impl Future<Output = Vec<EntityRef>> + Send;
}

/// One provider, then the rest.
#[derive(Debug)]
pub(crate) struct Chain<A, B>(pub(crate) A, pub(crate) B);

impl<P: IntentProvider> Builtins for P {
    fn serves(&self, app: &AppName) -> bool {
        self.manifest().manifest().app == *app
    }

    fn manifests<'a>(&'a self, out: &mut Vec<&'a ValidManifest>) {
        out.push(self.manifest());
    }

    fn hosted_perform(
        &self,
        _app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<Outcome, AppRefusal>> + Send {
        IntentProvider::perform(self, inv)
    }

    fn hosted_dry_run(
        &self,
        _app: &AppName,
        inv: Invocation,
    ) -> impl Future<Output = Result<Preview, AppRefusal>> + Send {
        IntentProvider::dry_run(self, inv)
    }

    fn hosted_undo(
        &self,
        _app: &AppName,
        token: UndoToken,
        actor: Actor,
    ) -> impl Future<Output = Result<(), UndoFault>> + Send {
        IntentProvider::undo(self, token, actor)
    }

    fn hosted_preview(
        &self,
        _app: &AppName,
        id: &EntityId,
    ) -> impl Future<Output = Preview> + Send {
        IntentProvider::preview(self, id)
    }

    fn hosted_suggest(
        &self,
        _app: &AppName,
        ask: SuggestAsk,
    ) -> impl Future<Output = Vec<EntityRef>> + Send {
        IntentProvider::suggest(self, ask)
    }
}

impl<A: Builtins, B: Builtins> Builtins for Chain<A, B> {
    fn serves(&self, app: &AppName) -> bool {
        self.0.serves(app) || self.1.serves(app)
    }

    fn manifests<'a>(&'a self, out: &mut Vec<&'a ValidManifest>) {
        self.0.manifests(out);
        self.1.manifests(out);
    }

    async fn hosted_perform(&self, app: &AppName, inv: Invocation) -> Result<Outcome, AppRefusal> {
        if self.0.serves(app) {
            self.0.hosted_perform(app, inv).await
        } else {
            self.1.hosted_perform(app, inv).await
        }
    }

    async fn hosted_dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        if self.0.serves(app) {
            self.0.hosted_dry_run(app, inv).await
        } else {
            self.1.hosted_dry_run(app, inv).await
        }
    }

    async fn hosted_undo(
        &self,
        app: &AppName,
        token: UndoToken,
        actor: Actor,
    ) -> Result<(), UndoFault> {
        if self.0.serves(app) {
            self.0.hosted_undo(app, token, actor).await
        } else {
            self.1.hosted_undo(app, token, actor).await
        }
    }

    async fn hosted_preview(&self, app: &AppName, id: &EntityId) -> Preview {
        if self.0.serves(app) {
            self.0.hosted_preview(app, id).await
        } else {
            self.1.hosted_preview(app, id).await
        }
    }

    async fn hosted_suggest(&self, app: &AppName, ask: SuggestAsk) -> Vec<EntityRef> {
        if self.0.serves(app) {
            self.0.hosted_suggest(app, ask).await
        } else {
            self.1.hosted_suggest(app, ask).await
        }
    }
}
