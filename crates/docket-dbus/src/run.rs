//! `org.quire.Intents1.Run`: performing, previewing, suggesting and undoing.

use porter_dbus::Details;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Run",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Run {
    /// Performs a call (`CallRequest` JSON; the window is `Option<WindowKey>` JSON). Answers a Request object whose `Response` carries the `Outcome` or `CallRefusal`.
    fn perform(
        &self,
        call: &str,
        parent_window: &str,
        options: &Details,
    ) -> zbus::Result<OwnedObjectPath>;

    /// A preview (`EntityId` JSON in, `Preview` JSON out).
    fn preview(&self, entity: &str) -> zbus::Result<String>;

    /// Suggested things (`SuggestAsk` JSON in, `Vec<EntityRef>` JSON out).
    fn suggest(&self, ask: &str) -> zbus::Result<String>;

    /// Undoes one journal row, as the person. Answers a Request.
    fn undo(&self, entry: u64) -> zbus::Result<OwnedObjectPath>;

    /// Undoes a run, a task or an entry (`UndoScope` JSON). Answers a Request.
    fn undo_all(&self, scope: &str) -> zbus::Result<OwnedObjectPath>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct RunSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Run")]
impl RunSkeleton {
    fn perform(
        &self,
        call: String,
        parent_window: String,
        options: Details,
    ) -> fdo::Result<OwnedObjectPath> {
        let _ = (call, parent_window, options);
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

    fn undo(&self, entry: u64) -> fdo::Result<OwnedObjectPath> {
        let _ = (entry,);
        Err(crate::introspect::frozen())
    }

    fn undo_all(&self, scope: String) -> fdo::Result<OwnedObjectPath> {
        let _ = (scope,);
        Err(crate::introspect::frozen())
    }
}
