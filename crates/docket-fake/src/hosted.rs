//! An app a test runs itself behind a bus name: the router's `Perform`, `DryRun` and `Undo` for it
//! reach this trait. What `FakeLink` does for `org.quire.Companion`, generalised to a provider
//! that is async and carries its own state (the external agents' host).

use docket_core::{AppRefusal, Invocation, Outcome, Preview, UndoFault, UndoToken};
use prov::Actor;
use std::future::Future;
use std::pin::Pin;

/// A boxed future, as a hosted app answers.
pub type BoxFut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// An app answered in the test's own process.
pub trait HostedApp: Send + Sync + std::fmt::Debug {
    /// `Perform`.
    fn perform(&self, inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>>;

    /// `DryRun`; unsupported unless the app says otherwise.
    fn dry_run(&self, inv: Invocation) -> BoxFut<Result<Preview, AppRefusal>> {
        let _ = inv;
        Box::pin(async { Err(AppRefusal::Unsupported) })
    }

    /// `Undo`; the app is unavailable unless it says otherwise.
    fn undo(&self, token: UndoToken, actor: Actor) -> BoxFut<Result<(), UndoFault>> {
        let _ = (token, actor);
        Box::pin(async { Err(UndoFault::AppUnavailable) })
    }
}
