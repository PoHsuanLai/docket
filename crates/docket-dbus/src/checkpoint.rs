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
}
