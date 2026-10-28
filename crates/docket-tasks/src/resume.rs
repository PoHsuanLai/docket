//! Restart. companiond keeps no state of its own: `recover` rebuilds the roster and the front task
//! from the eventlog, and this puts the result back to work: the agents that were working show on
//! the roster again (their goals are not in the digest, so presence only, plus what the person last
//! told them), and the front task gets a fresh session in the Space it was in.

use crate::best_effort::best_effort;
use crate::fault::ServeFault;
use crate::recover::{RouterRecent, rebuild_from, recent_events};
use crate::router_log::RouterLog;
use crate::runtime::Companion;
use crate::seams::Surface;
use crate::stored_roster::stored_events;
use agent_loop::Rebuilt;
use docket_client::Transport as IntentsTransport;
use docket_planner::Planner;
use docket_vocab::Clock;
use docket_wire::{RecallAsk, RecallView, SessionOpen, SessionOpened};
use prov::{AgentRef, SpaceId, UnixSeconds};

/// What a restart found: the front task's fresh session, if it had one, and how many of the
/// things it reads were read (`restored`) and how many were not (`failed`: the stored sessions,
/// and each Space's open and read). A restart that read nothing and failed everything is the
/// signal that the router or memoryd was away.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Restarted {
    /// The front task's fresh session.
    pub opened: Option<SessionOpened>,
    /// Reads that succeeded.
    pub restored: usize,
    /// Reads that failed.
    pub failed: usize,
}

/// How far back a restart reads: the retention window of `companion.*` records (30 days).
const RETENTION: i64 = 30 * 24 * 3600;

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// The Spaces to read on a restart: the ones `named` (the configuration's, the floor) and
    /// every Space the router says it knows (`Recall::Spaces`, asked in a session of the desktop
    /// that is closed again). A router that does not answer leaves the named ones.
    pub(crate) async fn spaces_to_read(&self, named: &[SpaceId]) -> Vec<SpaceId> {
        let mut spaces = named.to_vec();
        let Ok(asking) = self
            .intents
            .session_open(SessionOpen {
                space: SpaceId::desktop(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
                acts_for: None,
            })
            .await
        else {
            return spaces;
        };
        let known = self
            .intents
            .session_recall(asking.session.clone(), RecallAsk::Spaces)
            .await;
        let who = asking.session.clone();
        let closed = self.intents.session_close(asking.session).await;
        best_effort("session.close", &who, closed);
        if let Err(cause) = &known {
            tracing::warn!(%cause, "restart: the router would not list its Spaces");
        }
        if let Ok(RecallView::Spaces(known)) = known {
            for space in known {
                if !spaces.contains(&space) {
                    spaces.push(space);
                }
            }
        }
        spaces
    }

    /// A restart: reads the sessions the router stores (`Session.Stored`) and what the eventlog
    /// holds of each Space in `spaces` and each the router knows (through a session opened for the
    /// purpose, which is closed again), rebuilds the roster and the front task, and takes them
    /// up. A Space the router or memoryd cannot answer for adds nothing; sessions an editor
    /// opened are not the companion's and are left out.
    pub async fn restore(&mut self, spaces: &[SpaceId]) -> Result<Restarted, ServeFault> {
        let since = UnixSeconds(self.clock.now().0.saturating_sub(RETENTION));
        let (mut restored, mut failed) = (0, 0);
        let stored = match stored_events(&RouterLog::reading(&self.intents)).await {
            Ok(stored) => {
                restored += 1;
                stored
            }
            Err(cause) => {
                failed += 1;
                tracing::warn!(%cause, "restart: the stored sessions could not be read");
                None
            }
        };
        let mut recent = Vec::new();
        for space in &self.spaces_to_read(spaces).await {
            let reading = match self
                .intents
                .session_open(SessionOpen {
                    space: space.clone(),
                    agent: AgentRef::Companion,
                    parent: None,
                    cwd: None,
                    started_from: None,
                    external: None,
                    helper: None,
                    acts_for: None,
                })
                .await
            {
                Ok(reading) => reading,
                Err(cause) => {
                    failed += 1;
                    tracing::warn!(?space, %cause, "restart: no session to read the Space through");
                    continue;
                }
            };
            let found = recent_events(
                &RouterRecent::new(&self.intents, reading.session.clone()),
                since,
            )
            .await;
            let who = reading.session.clone();
            let closed = self.intents.session_close(reading.session).await;
            best_effort("session.close", &who, closed);
            match found {
                Ok(found) => {
                    restored += 1;
                    recent.extend(found);
                }
                Err(cause) => {
                    failed += 1;
                    tracing::warn!(?space, %cause, "restart: the Space's records could not be read");
                }
            }
        }
        let opened = self.resume(rebuild_from(stored, recent)).await?;
        Ok(Restarted {
            opened,
            restored,
            failed,
        })
    }

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
                    cwd: None,
                    started_from: None,
                    external: None,
                    helper: None,
                    acts_for: None,
                })
                .await?,
            ),
            None => None,
        };
        self.publish();
        Ok(opened)
    }
}
