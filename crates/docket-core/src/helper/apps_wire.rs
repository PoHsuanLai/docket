//! The host of the agent apps across a process boundary: the board's calls on [`AgentApps`] as
//! one request and one reply each, so a bus can carry them. The process that keeps the agents is
//! not the one that keeps the board (only the host of an outside agent may open its session at
//! the router), so [`AgentApps`] is a seam the board reaches over this.

use super::agent::SignInWay;
use super::apps::{AgentOutcome, CheckOutcome, SignInOutcome, SignOutOutcome};
use super::card::HelperCard;
use super::hand::{BackOutcome, BroughtOutcome, ToTerminalOutcome};
use super::job::Job;
use crate::hand::{SeenCommand, WatchedAgent};
use crate::workspace::Workspace;
use prov::{HelperId, SessionId};
use serde::{Deserialize, Serialize};

/// One call on the host of the agent apps.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "call", rename_all = "snake_case")]
pub enum AppsAsk {
    /// [`AgentApps::run`](super::AgentApps::run).
    Run {
        /// The helper.
        card: Box<HelperCard>,
        /// The job.
        job: Box<Job>,
    },
    /// [`AgentApps::stop`](super::AgentApps::stop).
    Stop {
        /// The helper.
        helper: HelperId,
    },
    /// [`AgentApps::sign_in`](super::AgentApps::sign_in).
    SignIn {
        /// The helper.
        card: Box<HelperCard>,
        /// The way chosen.
        way: SignInWay,
    },
    /// [`AgentApps::check`](super::AgentApps::check).
    Check {
        /// The helper.
        card: Box<HelperCard>,
    },
    /// [`AgentApps::sign_out`](super::AgentApps::sign_out).
    SignOut {
        /// The helper.
        card: Box<HelperCard>,
    },
    /// [`AgentApps::forget`](super::AgentApps::forget).
    Forget {
        /// The helper.
        helper: HelperId,
    },
    /// [`AgentApps::to_terminal`](super::AgentApps::to_terminal).
    ToTerminal {
        /// The helper.
        card: Box<HelperCard>,
    },
    /// [`AgentApps::bring_back`](super::AgentApps::bring_back).
    BringBack {
        /// The helper.
        card: Box<HelperCard>,
    },
    /// [`AgentApps::bring_back_watched`](super::AgentApps::bring_back_watched).
    BringBackWatched {
        /// The watch session.
        session: SessionId,
        /// The app and its own id for the conversation.
        agent: WatchedAgent,
        /// The folder the terminal ran the app in.
        folder: Workspace,
    },
    /// [`AgentApps::recognise`](super::AgentApps::recognise).
    Recognise {
        /// The command line a terminal saw in a pane.
        seen: SeenCommand,
    },
}

/// What the host answers, one reply for each call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "call", content = "v", rename_all = "snake_case")]
pub enum AppsReply {
    /// The job's end.
    Run(AgentOutcome),
    /// Stopped (or nothing was running).
    Stop,
    /// How the sign-in went.
    SignIn(SignInOutcome),
    /// Whether the agent is signed in.
    Check(CheckOutcome),
    /// How the sign-out went.
    SignOut(SignOutOutcome),
    /// The agent ended.
    Forget,
    /// How opening the conversation in a terminal went.
    ToTerminal(ToTerminalOutcome),
    /// How bringing the conversation back went.
    BringBack(BackOutcome),
    /// How bringing back a terminal's conversation went.
    BringBackWatched(BroughtOutcome),
    /// The app and conversation the command line picks up, when it is one.
    Recognise(Option<WatchedAgent>),
}
