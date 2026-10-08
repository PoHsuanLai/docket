//! The world a backend is tested in: the sheet the router's confirmer is (the desktop's, a script
//! of the person's answers, or the host's own desk for a session routed there), the host's
//! performer answering for the pseudo-app, and the seams that wire them.

use docket_acp::client::fake::{FakeFiles, FakeSpawn};
use docket_acp::client::{Files, IntentsCourt, OsFiles, Performer, Seams};
use docket_client::InProcess;
use docket_core::{
    AppRefusal, ConfirmAnswer, ConfirmId, ConfirmRequest, Confirmer, Invocation, Outcome,
    UndoFault, UndoToken,
};
use docket_fake::{
    BoxFut, FakeSeams, FixedClock, HostedApp, ScriptedConfirmer, ScriptedReviewer, ScriptedWriter,
};
use docket_inapp::{EditorDesk, SheetConfirmer};
use docket_session::fake::MemoryLog;
use docket_shell::fake::FakeSandbox;
use prov::Actor;
use std::sync::Arc;

/// The desktop's sheet (a script of the person's answers) for a call, and the host's own desk for
/// a session whose host asked to show its sheets: the route intentd makes.
pub struct DeskConfirmer {
    pub desktop: ScriptedConfirmer,
    pub(super) host: SheetConfirmer<EditorDesk, FixedClock>,
}

impl Confirmer for DeskConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        if request.editor.is_some() {
            self.host.confirm(request).await
        } else {
            self.desktop.confirm(request).await
        }
    }

    async fn cancel(&self, id: &ConfirmId) {
        self.desktop.cancel(id).await;
        self.host.cancel(id).await;
    }
}

pub type WorldSeams =
    FakeSeams<ScriptedReviewer, ScriptedWriter, FixedClock, Arc<MemoryLog>, DeskConfirmer>;
pub type World = docket_router::Router<WorldSeams>;
pub type TheCourt = IntentsCourt<InProcess<WorldSeams>>;

/// The host's performer, answering the router's `Perform` for the pseudo-app.
pub struct Hosted<F: Files + 'static>(pub Performer<F, FakeSandbox>);

impl<F: Files + 'static> std::fmt::Debug for Hosted<F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Hosted")
    }
}

impl<F: Files + 'static> HostedApp for Hosted<F> {
    fn perform(&self, inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>> {
        let done = self.0.perform(&inv);
        Box::pin(async move { done })
    }

    fn undo(&self, token: UndoToken, _actor: Actor) -> BoxFut<Result<(), UndoFault>> {
        let done = self.0.undo(&token);
        Box::pin(async move { done })
    }
}

pub struct Fakes;

impl Seams for Fakes {
    type Spawn = FakeSpawn;
    type Files = FakeFiles;
    type Court = TheCourt;
    type Sandbox = FakeSandbox;
}

pub struct Real;

impl Seams for Real {
    type Spawn = FakeSpawn;
    type Files = OsFiles;
    type Court = TheCourt;
    type Sandbox = FakeSandbox;
}
