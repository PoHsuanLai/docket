//! The seam between the board and the agent apps that are helpers: the host that keeps one agent
//! process per helper implements [`AgentApps`]; the hub calls it for a helper whose kind is
//! [`HelperKind::AgentApp`](super::HelperKind::AgentApp) where it would otherwise run its own
//! planner. Everything the agent does goes through the router; this carries only the job in and
//! the typed outcome out.

use super::agent::{SignInWay, TerminalLogin, UntrustedWords};
use super::hand::{BackOutcome, BroughtOutcome, HandFault, ToTerminalOutcome};
use crate::hand::WatchedAgent;
use crate::workspace::Workspace;
use super::card::HelperCard;
use super::job::{Job, JobFault};
use prov::{HelperId, SessionId, TaskId};
use serde::{Deserialize, Serialize};
use std::future::Future;
use std::pin::Pin;

/// A boxed future the seam's methods return.
pub type Pending<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// How a board job ended on an agent app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentOutcome {
    /// The agent answered. Its words are the agent model's: untrusted. The task and session are
    /// the agent's own at the router, from which the answer's label comes.
    Done {
        /// What it said.
        words: UntrustedWords,
        /// The task it ran as.
        task: TaskId,
        /// Its session at the router.
        session: SessionId,
        /// The app's own id for its conversation, when the person could pick it up in the
        /// app's own terminal program. Absent from an older host's answer.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        own_session: Option<String>,
    },
    /// The agent wants the person to sign in first. The job did not fail.
    NeedsSignIn(Vec<SignInWay>),
    /// The job did not finish.
    Failed(JobFault),
    /// The job was stopped before it finished.
    Stopped,
}

/// What choosing a way of signing in came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignInOutcome {
    /// The agent signed in.
    SignedIn,
    /// The shell is to open a terminal running the agent with this added.
    OpenTerminal(TerminalLogin),
    /// The agent would not take that way.
    Refused,
    /// The agent could not be started.
    Failed,
}

/// What signing out came to. An agent with no sign-out of its own is signed out with the logout
/// command of its entry in the agent list, which the implementation runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SignOutOutcome {
    /// Signed out.
    SignedOut,
    /// It could not be.
    Failed,
}

/// What asking the agent again after a sign-in in a terminal came to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CheckOutcome {
    /// The agent is signed in.
    SignedIn,
    /// Not yet: these are its ways.
    StillNeedsSignIn(Vec<SignInWay>),
    /// The agent could not be started.
    Failed,
}

/// The host of the agent apps that are helpers, one agent process per helper.
pub trait AgentApps: Send + Sync {
    /// Gives the helper's agent the job. Its goal is the turn's text and nothing else is sent:
    /// the goal is trusted, and no earlier answer travels in the prompt.
    fn run<'a>(&'a self, card: &'a HelperCard, job: &'a Job) -> Pending<'a, AgentOutcome>;

    /// Stops the helper's job in progress.
    fn stop<'a>(&'a self, helper: &'a HelperId) -> Pending<'a, ()>;

    /// Signs the helper's agent in with `way`, one it listed.
    fn sign_in<'a>(
        &'a self,
        card: &'a HelperCard,
        way: &'a SignInWay,
    ) -> Pending<'a, SignInOutcome>;

    /// Asks the helper's agent again whether it is signed in, after the person signed in in the
    /// terminal the shell opened.
    fn check<'a>(&'a self, card: &'a HelperCard) -> Pending<'a, CheckOutcome>;

    /// Signs the helper's agent out.
    fn sign_out<'a>(&'a self, card: &'a HelperCard) -> Pending<'a, SignOutOutcome>;

    /// The helper was removed: its agent process ends.
    fn forget<'a>(&'a self, helper: &'a HelperId) -> Pending<'a, ()>;

    /// Closes the helper's agent and turns its session at the router into a watch session, and
    /// answers the command that picks the conversation up in the app's own terminal program.
    /// A host without the means refuses.
    fn to_terminal<'a>(&'a self, card: &'a HelperCard) -> Pending<'a, ToTerminalOutcome> {
        let _ = card;
        Box::pin(async { ToTerminalOutcome::Refused(HandFault::Failed) })
    }

    /// Starts the helper's agent again on the session it left, on the conversation the
    /// terminal went on with.
    fn bring_back<'a>(&'a self, card: &'a HelperCard) -> Pending<'a, BackOutcome> {
        let _ = card;
        Box::pin(async { BackOutcome::Refused(HandFault::Failed) })
    }

    /// Starts `agent` on the watch session `session`, which a terminal opened for it in `folder`,
    /// as a conversation of docket's.
    fn bring_back_watched<'a>(
        &'a self,
        session: &'a prov::SessionId,
        agent: &'a WatchedAgent,
        folder: &'a Workspace,
    ) -> Pending<'a, BroughtOutcome> {
        let _ = (session, agent, folder);
        Box::pin(async { BroughtOutcome::Refused(HandFault::Failed) })
    }
}
