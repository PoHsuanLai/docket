//! `org.quire.Intents1.Control`: halting, resuming and the journal.

use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Control",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Control {
    /// Halts a scope (`SpaceScope`, `HaltCause` JSON).
    fn halt(&self, scope: &str, cause: &str) -> zbus::Result<()>;

    /// Resumes a scope. Control role only.
    fn resume(&self, scope: &str) -> zbus::Result<()>;

    /// The kill switch (`KillSwitch` JSON).
    fn state(&self) -> zbus::Result<String>;

    /// The undo journal (`JournalFilter` JSON; answers `Vec<UndoEntry>` JSON).
    fn journal(&self, filter: &str) -> zbus::Result<String>;

    /// The actions the terminal (`quire-do`) may run without asking, until logout
    /// (`Vec<ActionRef>` JSON). Control role only.
    fn terminal_grants(&self) -> zbus::Result<String>;

    /// Withdraws one standing terminal grant (`ActionRef` JSON). Control role only.
    fn revoke_terminal_grant(&self, action: &str) -> zbus::Result<()>;

    /// A scope was halted.
    #[zbus(signal)]
    fn halted(&self, scope: &str) -> zbus::Result<()>;

    /// A scope was resumed.
    #[zbus(signal)]
    fn resumed(&self, scope: &str) -> zbus::Result<()>;

    /// The journal changed; this many rows.
    #[zbus(signal)]
    fn journal_changed(&self, rows: u64) -> zbus::Result<()>;

    /// The breaker paused a session until the person speaks.
    #[zbus(signal)]
    fn breaker_tripped(&self, session: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ControlSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Control")]
impl ControlSkeleton {
    fn halt(&self, scope: String, cause: String) -> fdo::Result<()> {
        let _ = (scope, cause);
        Err(crate::introspect::frozen())
    }

    fn resume(&self, scope: String) -> fdo::Result<()> {
        let _ = (scope,);
        Err(crate::introspect::frozen())
    }

    fn state(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn journal(&self, filter: String) -> fdo::Result<String> {
        let _ = (filter,);
        Err(crate::introspect::frozen())
    }

    fn terminal_grants(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }

    fn revoke_terminal_grant(&self, action: String) -> fdo::Result<()> {
        let _ = (action,);
        Err(crate::introspect::frozen())
    }

    #[zbus(signal)]
    async fn halted(emitter: &SignalEmitter<'_>, scope: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn resumed(emitter: &SignalEmitter<'_>, scope: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn journal_changed(emitter: &SignalEmitter<'_>, rows: u64) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn breaker_tripped(emitter: &SignalEmitter<'_>, session: &str) -> zbus::Result<()>;
}
