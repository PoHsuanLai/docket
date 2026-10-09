//! Recording the person's words for an agent the app runs by itself (a `docket-kit` agent over
//! [`InAppAgent::companion_link`]). The `field` link alone records, as for the host's own tasks;
//! the kit's link stays `companion`, so the two roles are never one caller.

use crate::agent::{AgentFault, InAppAgent};
use crate::sheet::ConfirmSheet;
use action_review::Reviewer;
use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    ContextKeep, Keep, Origin, PolicyWriter, Reader, SessionOpen, TurnIn, TurnSource, TurnVia,
    UserTurn,
};
use docket_router::{Clock, GrantStore, MemoryLink};
use porter_client::Transport as ModelTransport;
use prov::{AgentRef, SessionId};

/// The person's words, recorded by the host in a session it opened. Hand `session` and `turn` to
/// the kit's `Agent::ask_recorded`, and give the record back to [`InAppAgent::end_recorded`].
#[derive(Debug, Clone, PartialEq)]
pub struct RecordedTurn {
    /// The session the turn is recorded in (the host's, opened as the app).
    pub session: SessionId,
    /// The turn, as the router recorded it: the person's words from the app's own prompt.
    pub turn: UserTurn,
}

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    /// Opens a session and records `text` in it as the person's turn, through the host's `field`
    /// link. Nothing runs: the app hands the result to a kit agent over
    /// [`InAppAgent::companion_link`].
    pub async fn record_turn(&self, text: &str) -> Result<RecordedTurn, AgentFault> {
        let opened = self
            .person
            .session_open(SessionOpen {
                space: self.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            })
            .await?;
        let id = self
            .person
            .session_turn(
                opened.session.clone(),
                TurnIn {
                    text: text.to_owned(),
                    origin: Origin::InWindowField,
                    keep: ContextKeep {
                        query: Keep::Dropped,
                        results: Keep::Dropped,
                        selection: Keep::Dropped,
                        window: Keep::Dropped,
                    },
                    via: TurnVia::Typed,
                },
            )
            .await?;
        Ok(RecordedTurn {
            session: opened.session,
            turn: UserTurn {
                id,
                text: text.to_owned(),
                at: self.clock.now(),
                from: TurnSource::Field(self.app.clone()),
                via: TurnVia::Typed,
            },
        })
    }

    /// Closes the session of a recorded turn once the kit agent is done with it.
    pub async fn end_recorded(&self, recorded: RecordedTurn) -> Result<(), AgentFault> {
        Ok(self.person.session_close(recorded.session).await?)
    }
}
