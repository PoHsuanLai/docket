//! The seam between the host of an external agent and the router. Every call the agent asks of
//! its host (a file read or write, a command, a permission request) is made through it as an
//! ordinary router call of the `org.quire.AcpAgent` pseudo-app (`call`), and the host performs it
//! only after the router has allowed it. The host decides nothing: not whether to ask the
//! person, not whether a grant stands in, not whether the breaker is tripped.
//!
//! What the host does before a call is form it: a path is confined to the session's directory
//! (links and secrets included) before the call exists, because a path outside is not a call
//! the router should be asked to weigh.

use super::call::AgentCall;
use docket_core::{
    CallRefusal, ExternalAgent, Outcome, Rewind, SheetSurface, TurnEnd, TurnId, ValidManifest,
};
use docket_session::{ProgramName, Workspace};
use prov::{SessionId, SpaceId};
use std::future::Future;

/// How the router ruled on a call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ruled {
    /// It went ahead, and this is what the host performed (a read's text, a terminal's name).
    Done(Box<Outcome>),
    /// It did not.
    Refused(CallRefusal),
    /// The router could not be asked or did not answer: nothing was done.
    Lost,
}

/// Why the router would not open or follow a session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum CourtFault {
    /// The router refused the request.
    #[error("the router refused")]
    Refused,
    /// The router could not be reached.
    #[error("the router could not be reached")]
    Unavailable,
}

/// What the host tells the router when it opens an agent's session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAgent {
    /// The configured program, as the host launched it.
    pub program: ProgramName,
    /// The directory the agent works in.
    pub cwd: Workspace,
    /// Where the person answers its sheets.
    pub sheets: SheetSurface,
    /// The Space the session is in: the one whose data its calls reach.
    pub space: SpaceId,
    /// What the person calls the agent (`agents.toml`), which only the host says: never the
    /// agent's own title from `initialize`.
    pub label: Option<prov::AgentLabel>,
    /// Who keeps the history of the agent's file changes (`checkpoints` in `agents.toml`), which
    /// only the host says: never the agent.
    pub rewind: Rewind,
}

impl OpenAgent {
    /// The agent the router is told of.
    pub fn external(&self) -> ExternalAgent {
        ExternalAgent {
            program: self.program.clone(),
            sheets: self.sheets,
            label: self.label.clone(),
            rewind: self.rewind,
        }
    }
}

/// The router, as the host of an external agent sees it. `Clone` because the host and the
/// backend each hold one.
///
/// **Cancel safety.** The backend may drop the future of `call` and ask again with the same `n`
/// (the call's number in this connection); an implementation must then go on waiting for the
/// one call and make no second one, or the person would see two sheets and the command would run
/// twice.
pub trait Court: Send + Clone + 'static {
    /// Opens the agent's session.
    fn open(
        &mut self,
        open: OpenAgent,
    ) -> impl Future<Output = Result<SessionId, CourtFault>> + Send;

    /// Records the person's turn, from which the task policy derives, and says which turn it is.
    /// May wait for the person (a turn that widens the task asks).
    fn turn(
        &mut self,
        session: &SessionId,
        text: &str,
    ) -> impl Future<Output = Result<TurnId, CourtFault>> + Send;

    /// Says the agent's turn is over, however it ended (`Session.TurnEnded`). A router that
    /// cannot be told ends the turn by time. The default says nothing.
    fn turn_ended(
        &mut self,
        _session: &SessionId,
        _turn: TurnId,
        _how: TurnEnd,
    ) -> impl Future<Output = ()> + Send {
        async {}
    }

    /// Makes one call as the router rules.
    fn call(
        &mut self,
        session: &SessionId,
        n: u64,
        call: &AgentCall,
    ) -> impl Future<Output = Ruled> + Send;

    /// The registry's manifests, for the tools the agent's edge offers.
    fn registry(&mut self) -> impl Future<Output = Option<Vec<ValidManifest>>> + Send;

    /// Ends the session.
    fn close(&mut self, session: &SessionId) -> impl Future<Output = ()> + Send;
}
