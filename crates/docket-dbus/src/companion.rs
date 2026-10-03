//! `org.quire.Companion1`: the companion, served by companiond: one identity over many tasks.

use porter_dbus::Details;
use zbus::fdo;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Companion1",
    default_service = "org.quire.Companion1",
    default_path = "/org/quire/Companion1"
)]
pub trait Companion {
    /// On summon: warms the planner model so the first turn skips a cold start.
    fn prepare(&self, options: &Details) -> zbus::Result<()>;

    /// Opens a session for a front conversation (`SessionOpen` JSON; answers `SessionOpened` JSON).
    fn open(&self, open: &str) -> zbus::Result<String>;

    /// Asks (`AskWire` JSON: a session, the turn the UI recorded with its context keep, the window and the app the person asked from). Answers the answer object.
    fn ask(&self, ask: &str, options: &Details) -> zbus::Result<OwnedObjectPath>;

    /// Closes a session.
    fn close(&self, session: &str) -> zbus::Result<()>;

    /// The person said something to a subagent through the shell (`AgentRef` JSON, `SpaceId`, `UserTurn` JSON): the companion keeps their words and shows the roster line at once.
    fn told(&self, agent: &str, space: &str, turn: &str) -> zbus::Result<()>;

    /// The roster (`Roster` JSON): who is working, one line each; another Space shows presence only.
    fn roster(&self) -> zbus::Result<String>;

    /// The task the launcher returns to (`FrontTask` JSON).
    fn front(&self) -> zbus::Result<String>;

    /// An answer object appeared.
    #[zbus(signal)]
    fn answer_added(&self, answer: ObjectPath<'_>) -> zbus::Result<()>;

    /// An answer object went away.
    #[zbus(signal)]
    fn answer_removed(&self, answer: ObjectPath<'_>) -> zbus::Result<()>;

    /// The roster changed. Content-free: read `Roster`.
    #[zbus(signal)]
    fn roster_changed(&self) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CompanionSkeleton;

#[zbus::interface(name = "org.quire.Companion1")]
impl CompanionSkeleton {
    fn prepare(&self, options: Details) -> fdo::Result<()> {
        let _ = (options,);
        Err(crate::introspect::frozen())
    }

    fn open(&self, open: String) -> fdo::Result<String> {
        let _ = (open,);
        Err(crate::introspect::frozen())
    }

    fn ask(&self, ask: String, options: Details) -> fdo::Result<OwnedObjectPath> {
        let _ = (ask, options);
        Err(crate::introspect::frozen())
    }

    fn close(&self, session: String) -> fdo::Result<()> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn told(&self, agent: String, space: String, turn: String) -> fdo::Result<()> {
        let _ = (agent, space, turn);
        Err(crate::introspect::frozen())
    }

    fn roster(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn front(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn answer_added(emitter: &SignalEmitter<'_>, answer: ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn answer_removed(
        emitter: &SignalEmitter<'_>,
        answer: ObjectPath<'_>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn roster_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
