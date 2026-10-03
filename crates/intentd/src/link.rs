//! The apps, over D-Bus: `IntentProvider1` on each app's own bus name. The router calls it only
//! after checking that the name's owner derives to the same `AppId`.

use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, EntityRef, Generation, Hit, Invocation, Latency,
    Outcome, Preview, SuggestAsk, UndoFault, UndoToken,
};
use docket_router::{AppFault, AppLink, LinkFault};
use porter_core::AppName;
use prov::{Actor, EntityId};

/// `AppLink` over the session bus.
#[derive(Debug, Clone)]
pub struct DbusLink {
    connection: docket_dbus::BusConnection,
}

impl DbusLink {
    /// Calls apps over `connection`.
    pub fn new(connection: docket_dbus::BusConnection) -> Self {
        Self { connection }
    }
}

impl AppLink for DbusLink {
    async fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        let _ = (&self.connection, app, inv, within);
        todo!(
            "DbusLink::perform: IntentProviderProxy for the app's name, within 250 ms, 5 s or a progress request by latency; a timeout is AppFault::TimedOut, an absent app is activated first"
        )
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        let _ = (&self.connection, app, inv);
        todo!("DbusLink::dry_run: IntentProvider1.DryRun")
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        let _ = (&self.connection, app, token, actor);
        todo!("DbusLink::undo: IntentProvider1.Undo")
    }

    async fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        let _ = (&self.connection, app, scope);
        todo!("DbusLink::context: IntentProvider1.Context")
    }

    async fn search(
        &self,
        app: &AppName,
        text: &str,
        generation: Generation,
    ) -> Result<Vec<Hit>, LinkFault> {
        let _ = (&self.connection, app, text, generation);
        todo!("DbusLink::search: IntentProvider1.Search within 250 ms")
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        let _ = (&self.connection, app, id);
        todo!("DbusLink::preview: IntentProvider1.Preview")
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        let _ = (&self.connection, app, ask);
        todo!("DbusLink::suggest: IntentProvider1.Suggest")
    }
}
