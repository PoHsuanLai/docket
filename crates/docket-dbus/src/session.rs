//! `org.quire.Intents1.Session`: sessions: one per task. The role decides who may call what.

use porter_dbus::Details;
use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Session",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Session {
    /// Opens a session (`SessionOpen` JSON; answers `SessionOpened` JSON).
    fn open(&self, open: &str) -> zbus::Result<String>;

    /// Records the person's turn (`TurnIn` JSON; answers the `TurnId`). Launcher and field roles only.
    fn turn(&self, session: &str, turn: &str, options: &Details) -> zbus::Result<u64>;

    /// Closes a session.
    fn close(&self, session: &str) -> zbus::Result<()>;

    /// The host says the agent's turn is over (`TurnEnd` JSON for how it ended). Same roles as `Turn`.
    fn turn_ended(&self, session: &str, turn: u64, how: &str) -> zbus::Result<()>;

    /// The text behind a handle, for the reader only.
    fn resolve(&self, session: &str, handle: u64) -> zbus::Result<String>;

    /// The text behind a handle, for the screen: never for a model.
    fn display(&self, session: &str, handle: u64) -> zbus::Result<String>;

    /// The text behind a handle with its label, for the screen: never for a model (`Displayed`
    /// JSON). Same roles as `Display`; the label lets the screen keep the trust mark.
    fn display_labelled(&self, session: &str, handle: u64) -> zbus::Result<String>;

    /// The planner asks the reader (`ReaderAsk` JSON; answers `Reveal<Value>` JSON).
    fn read(&self, session: &str, ask: &str, options: &Details) -> zbus::Result<String>;

    /// The task policy (`Option<TaskPolicy>` JSON).
    fn task_policy(&self, session: &str) -> zbus::Result<String>;

    /// Asks to widen the task policy (`TaskPolicy` JSON); the person confirms against their turn. Answers a Request.
    fn widen(&self, session: &str, turn: u64, change: &str) -> zbus::Result<OwnedObjectPath>;

    /// Hands the router something to record (`NoteAsk` JSON: an episode, a narrative of one the router holds, or a record of the companion's sessions). Companion role only.
    fn note(&self, session: &str, note: &str) -> zbus::Result<()>;

    /// Narrows a subagent's task policy from what the person said to it (`UserTurn` JSON): never wider than the policy it has. Companion role only.
    fn narrow(&self, session: &str, turn: &str) -> zbus::Result<()>;

    /// What the session holds by handle, in shape only (`Vec<HandleCard>` JSON). Companion role only.
    fn handles(&self, session: &str) -> zbus::Result<String>;

    /// Reads memory for the session (`RecallAsk` JSON; answers `RecallView` JSON), labels applied by the router.
    fn recall(&self, session: &str, ask: &str, options: &Details) -> zbus::Result<String>;

    /// The durable log of sessions (`StoredAsk` JSON; answers `StoredView` JSON): the sessions the caller may bring back, a page of one's rows, or a fork of one (the router writes the child).
    fn stored(&self, ask: &str) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct SessionSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Session")]
impl SessionSkeleton {
    fn open(&self, open: String) -> fdo::Result<String> {
        let _ = (open,);
        Err(crate::introspect::frozen())
    }

    fn turn(&self, session: String, turn: String, options: Details) -> fdo::Result<u64> {
        let _ = (session, turn, options);
        Err(crate::introspect::frozen())
    }

    fn close(&self, session: String) -> fdo::Result<()> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn turn_ended(&self, session: String, turn: u64, how: String) -> fdo::Result<()> {
        let _ = (session, turn, how);
        Err(crate::introspect::frozen())
    }

    fn resolve(&self, session: String, handle: u64) -> fdo::Result<String> {
        let _ = (session, handle);
        Err(crate::introspect::frozen())
    }

    fn display(&self, session: String, handle: u64) -> fdo::Result<String> {
        let _ = (session, handle);
        Err(crate::introspect::frozen())
    }

    fn display_labelled(&self, session: String, handle: u64) -> fdo::Result<String> {
        let _ = (session, handle);
        Err(crate::introspect::frozen())
    }

    fn read(&self, session: String, ask: String, options: Details) -> fdo::Result<String> {
        let _ = (session, ask, options);
        Err(crate::introspect::frozen())
    }

    fn task_policy(&self, session: String) -> fdo::Result<String> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn widen(&self, session: String, turn: u64, change: String) -> fdo::Result<OwnedObjectPath> {
        let _ = (session, turn, change);
        Err(crate::introspect::frozen())
    }

    fn note(&self, session: String, note: String) -> fdo::Result<()> {
        let _ = (session, note);
        Err(crate::introspect::frozen())
    }

    fn narrow(&self, session: String, turn: String) -> fdo::Result<()> {
        let _ = (session, turn);
        Err(crate::introspect::frozen())
    }

    fn handles(&self, session: String) -> fdo::Result<String> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn recall(&self, session: String, ask: String, options: Details) -> fdo::Result<String> {
        let _ = (session, ask, options);
        Err(crate::introspect::frozen())
    }

    fn stored(&self, ask: String) -> fdo::Result<String> {
        let _ = (ask,);
        Err(crate::introspect::frozen())
    }
}
