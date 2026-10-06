//! `org.quire.IntentProvider1`: what every provider (an app, `org.quire.Shell`, `org.quire.Cua`) serves on its own bus name.

use porter_dbus::Details;
use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.IntentProvider1",
    default_path = "/org/quire/IntentProvider1"
)]
pub trait IntentProvider {
    /// Performs (`Invocation` JSON, flattened with an optional `activation` token and, for an action that classifies per call, the `classified` effect the router gated on; answers `Outcome` or `AppRefusal` JSON). The app labels its undo entry with the invocation's actor.
    fn perform(&self, invocation: &str, options: &Details) -> zbus::Result<String>;

    /// What one call does, for an action that declares `per_call = "classified"` (`Invocation` JSON; answers `CallClass` or `AppRefusal` JSON). The declared effect is a ceiling: the router clamps the answer, and an error, a timeout or an unknown method leaves the ceiling in force.
    fn classify(&self, invocation: &str, options: &Details) -> zbus::Result<String>;

    /// Describes the change without making it (`Preview` JSON).
    fn dry_run(&self, invocation: &str, options: &Details) -> zbus::Result<String>;

    /// Undoes (`UndoToken`, `Actor` JSON; answers `()` or `UndoFault`).
    fn undo(&self, token: &str, actor: &str) -> zbus::Result<String>;

    /// What the person is looking at (`ContextSnapshot` JSON), answered by ds without app code.
    fn context(&self, scope: &str) -> zbus::Result<String>;

    /// The companion was summoned (`SummonOrigin` JSON; answers `SummonAnswer`).
    fn summon(&self, serial: u64, origin: &str) -> zbus::Result<String>;

    /// Search in kinds the app does not index.
    fn search(&self, text: &str, generation: u64) -> zbus::Result<String>;

    /// A preview (`EntityId` JSON in, `Preview` JSON out).
    fn preview(&self, entity: &str) -> zbus::Result<String>;

    /// Suggested things for a parameter.
    fn suggest(&self, ask: &str) -> zbus::Result<String>;

    /// Metadata for entity keys (the ids-then-metas split).
    fn resolve(&self, keys: &str) -> zbus::Result<String>;

    /// The app's own Cmd+Z changed an entry (`UndoState` JSON).
    #[zbus(signal)]
    fn undo_changed(&self, token: &str, state: &str) -> zbus::Result<()>;

    /// The app asks the router to request a `Reset`.
    #[zbus(signal)]
    fn index_stale(&self, epoch: u64) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct IntentProviderSkeleton;

#[zbus::interface(name = "org.quire.IntentProvider1")]
impl IntentProviderSkeleton {
    fn perform(&self, invocation: String, options: Details) -> fdo::Result<String> {
        let _ = (invocation, options);
        Err(crate::introspect::frozen())
    }

    fn classify(&self, invocation: String, options: Details) -> fdo::Result<String> {
        let _ = (invocation, options);
        Err(crate::introspect::frozen())
    }

    fn dry_run(&self, invocation: String, options: Details) -> fdo::Result<String> {
        let _ = (invocation, options);
        Err(crate::introspect::frozen())
    }

    fn undo(&self, token: String, actor: String) -> fdo::Result<String> {
        let _ = (token, actor);
        Err(crate::introspect::frozen())
    }

    fn context(&self, scope: String) -> fdo::Result<String> {
        let _ = (scope,);
        Err(crate::introspect::frozen())
    }

    fn summon(&self, serial: u64, origin: String) -> fdo::Result<String> {
        let _ = (serial, origin);
        Err(crate::introspect::frozen())
    }

    fn search(&self, text: String, generation: u64) -> fdo::Result<String> {
        let _ = (text, generation);
        Err(crate::introspect::frozen())
    }

    fn preview(&self, entity: String) -> fdo::Result<String> {
        let _ = (entity,);
        Err(crate::introspect::frozen())
    }

    fn suggest(&self, ask: String) -> fdo::Result<String> {
        let _ = (ask,);
        Err(crate::introspect::frozen())
    }

    fn resolve(&self, keys: String) -> fdo::Result<String> {
        let _ = (keys,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn undo_changed(
        emitter: &SignalEmitter<'_>,
        token: &str,
        state: &str,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn index_stale(emitter: &SignalEmitter<'_>, epoch: u64) -> zbus::Result<()>;
}
