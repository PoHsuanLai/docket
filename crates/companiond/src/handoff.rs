//! Giving a turn to the agent host over the session bus: `org.quire.AgentHost1`, served by
//! `docket-agent --serve` under the name `org.quire.AcpAgent`. Asking the name starts the host if
//! it is not running. The host opens the agent's session and makes its calls; this only hands it
//! the person's turn and waits for the agent to finish it.

use docket_core::{HandOver, HandRefusal, HandReply};
use docket_dbus::{AgentHostProxy, BusConnection};
use docket_tasks::Handoff;
use std::future::Future;
use std::pin::Pin;

/// The agent host, on the session bus.
#[derive(Debug, Clone)]
pub struct HostLink {
    connection: BusConnection,
}

impl HostLink {
    /// A link over `connection`.
    pub fn new(connection: BusConnection) -> Self {
        Self { connection }
    }

    async fn run(&self, ask: HandOver) -> HandReply {
        let unreachable = || {
            HandReply::Refused(HandRefusal::Failed {
                message: "The coding agent can't be reached.".to_owned(),
            })
        };
        let Ok(body) = serde_json::to_string(&ask) else {
            return unreachable();
        };
        let Ok(host) = AgentHostProxy::new(&self.connection).await else {
            return unreachable();
        };
        let Ok(turn) = host.hand_over(&body).await else {
            return unreachable();
        };
        // The host answers each wait within a minute with the turn still running or its reply.
        loop {
            match host.wait(&turn).await {
                Ok(text) if text.is_empty() => continue,
                Ok(text) => return serde_json::from_str(&text).unwrap_or_else(|_| unreachable()),
                Err(_) => return unreachable(),
            }
        }
    }
}

impl Handoff for HostLink {
    fn hand_over(&self, ask: HandOver) -> Pin<Box<dyn Future<Output = HandReply> + Send + '_>> {
        Box::pin(self.run(ask))
    }
}
