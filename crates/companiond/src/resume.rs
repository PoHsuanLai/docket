//! Restart. companiond keeps no state of its own: `recover` rebuilds the roster and the front task
//! from the eventlog, and this puts the result back to work: the agents that were working show on
//! the roster again (their goals are not in the digest, so presence only, plus what the person last
//! told them), and the front task gets a fresh session in the Space it was in.

use crate::fault::ServeFault;
use crate::runtime::Companiond;
use agent_loop::Rebuilt;
use docket_client::Transport as IntentsTransport;
use docket_core::{SessionOpen, SessionOpened};
use porter_client::Transport as InferTransport;
use prov::AgentRef;

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// Takes up what a restart rebuilt. A front task that was open is opened again as a fresh
    /// session (the old one died with the process or is the router's to close); nothing else is
    /// guessed.
    pub async fn resume(&mut self, rebuilt: Rebuilt) -> Result<Option<SessionOpened>, ServeFault> {
        for line in rebuilt.roster().entries {
            self.known.insert(line.agent.clone(), line);
        }
        let space = rebuilt
            .front
            .as_ref()
            .and_then(|front| {
                rebuilt
                    .tasks
                    .iter()
                    .find(|t| t.task.as_ref() == Some(front))
            })
            .map(|t| t.space.clone());
        let opened = match space {
            Some(space) => Some(
                self.open(SessionOpen {
                    space,
                    agent: AgentRef::Companion,
                    parent: None,
                })
                .await?,
            ),
            None => None,
        };
        self.publish();
        Ok(opened)
    }
}
