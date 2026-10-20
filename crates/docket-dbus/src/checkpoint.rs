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

    /// `Watch` for a terminal that also names the agent app it watches and the app's own id for
    /// the conversation (`agent` is `WatchedAgent` JSON), so the conversation can be brought into
    /// docket. Answers `Watching` JSON: the watch session and who closes it (the companion's own
    /// session when it handed this conversation over, else a new one).
    fn watch_agent(
        &self,
        workspace: &str,
        label: &str,
        rewind: &str,
        agent: &str,
    ) -> zbus::Result<String>;

    /// The host of the agent apps moves a session's conversation (`to` is `HandTo` JSON; `agent`
    /// is `WatchedAgent` JSON when it goes to the terminal, and the empty text otherwise).
    /// Answers the `WatchedAgent` JSON the router holds for it, or `null`.
    fn handover(&self, session: &str, to: &str, agent: &str) -> zbus::Result<String>;

    /// The open watch sessions whose agent app a terminal named (`WatchedLine` JSON list).
    fn watched(&self) -> zbus::Result<String>;
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

    fn watch_agent(
        &self,
        workspace: String,
        label: String,
        rewind: String,
        agent: String,
    ) -> fdo::Result<String> {
        let _ = (workspace, label, rewind, agent);
        Err(crate::introspect::frozen())
    }

    fn handover(&self, session: String, to: String, agent: String) -> fdo::Result<String> {
        let _ = (session, to, agent);
        Err(crate::introspect::frozen())
    }

    fn watched(&self) -> fdo::Result<String> {
        Err(crate::introspect::frozen())
    }
}
