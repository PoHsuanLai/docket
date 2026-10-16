//! `org.quire.AgentHost1`: the host of outside coding agents (`docket-agent`), which companiond
//! asks to give the person's turn to the agent they chose as the assistant's model.

use zbus::fdo;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.AgentHost1",
    default_service = "org.quire.AcpAgent",
    default_path = "/org/quire/AgentHost1"
)]
pub trait AgentHost {
    /// Gives a turn to an agent (`HandOver` JSON) and answers at once with the turn's id. The
    /// agent's calls on the way are the router's, ruled and asked about as any.
    fn hand_over(&self, handover: &str) -> zbus::Result<String>;

    /// Waits a while for the turn `turn` to end. Answers the empty text while the agent is still
    /// working (ask again), and once it has finished, the `HandReply` JSON (once: the turn is
    /// then forgotten).
    fn wait(&self, turn: &str) -> zbus::Result<String>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct AgentHostSkeleton;

#[zbus::interface(name = "org.quire.AgentHost1")]
impl AgentHostSkeleton {
    fn hand_over(&self, handover: String) -> fdo::Result<String> {
        let _ = (handover,);
        Err(crate::introspect::frozen())
    }

    fn wait(&self, turn: String) -> fdo::Result<String> {
        let _ = (turn,);
        Err(crate::introspect::frozen())
    }
}
