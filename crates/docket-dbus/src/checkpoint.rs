//! `org.quire.Intents1.Checkpoint`: a session's restore points and what restoring one changes.
//! Restoring itself is the action `checkpoints.restore`, through `Run.Perform` and the sheet.

use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Intents1.Checkpoint",
    default_service = "org.quire.Intents1",
    default_path = "/org/quire/Intents1"
)]
pub trait Checkpoint {
    /// The session's restore points, oldest first (`CheckpointList` JSON).
    fn list(&self, session: &str) -> zbus::Result<String>;

    /// What restoring `point` would change (`Result<RestorePlan, CheckpointFault>` JSON). Read-only.
    fn plan(&self, session: &str, point: u32) -> zbus::Result<String>;

    /// Opens a session that only keeps restore points for an agent the caller watches
    /// (`rewind` is `Rewind` JSON; `label` is display text). The new session id as JSON.
    fn watch(&self, workspace: &str, label: &str, rewind: &str) -> zbus::Result<String>;

    /// The watched agent started working: takes a restore point and starts the running turn;
    /// answers the `TurnId` for `Session.TurnEnded`.
    fn mark(&self, session: &str) -> zbus::Result<u64>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct CheckpointSkeleton;

#[zbus::interface(name = "org.quire.Intents1.Checkpoint")]
impl CheckpointSkeleton {
    fn list(&self, session: String) -> fdo::Result<String> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }

    fn plan(&self, session: String, point: u32) -> fdo::Result<String> {
        let _ = (session, point);
        Err(crate::introspect::frozen())
    }

    fn watch(&self, workspace: String, label: String, rewind: String) -> fdo::Result<String> {
        let _ = (workspace, label, rewind);
        Err(crate::introspect::frozen())
    }

    fn mark(&self, session: String) -> fdo::Result<u64> {
        let _ = (session,);
        Err(crate::introspect::frozen())
    }
}
