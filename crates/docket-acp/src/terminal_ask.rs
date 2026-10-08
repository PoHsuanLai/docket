//! The vocabulary of the terminal methods: what the person is asked, what they may answer, what
//! the host knows about the session's trust, and the notes kept for the audit.

use docket_core::{
    AbsPath, AlwaysOffer, ArgOrigin, BreakerState, BudgetState, ExecuteAsk, StandingGrantId,
    VerdictKind,
};
use std::future::Future;

/// A command waiting for the person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TerminalAsk {
    /// The literal command line (program and arguments).
    pub line: String,
    /// Where it would run.
    pub cwd: AbsPath,
    /// Why the person is asked.
    pub why: ExecuteAsk,
    /// Whether "always" may be offered, and for what.
    pub offer: AlwaysOffer,
}

/// The person's answer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Run it this once.
    Once,
    /// Run it, and remember the offered scope. Counts as `Once` where no scope was offered.
    Always,
    /// Do not run it.
    No,
}

/// Whoever asks the person: the editor's permission prompt, or our own sheet.
pub trait Decide: Send {
    /// Asks, and waits for the answer.
    fn decide(&mut self, ask: &TerminalAsk) -> impl Future<Output = Answer> + Send;
}

/// What the host knows about the session around the next command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Posture {
    /// Whether the command's arguments derive from untrusted content (the session is tainted).
    pub origin: ArgOrigin,
    /// The breaker.
    pub breaker: BreakerState,
    /// The budgets.
    pub budget: BudgetState,
    /// The reviewer's verdict on the next command, if one ran.
    pub review: Option<VerdictKind>,
}

impl Default for Posture {
    fn default() -> Self {
        Self {
            origin: ArgOrigin::Typed,
            breaker: BreakerState::Running,
            budget: BudgetState::Within,
            review: None,
        }
    }
}

/// What happened, for the audit. Commands and working directories appear; the environment never
/// does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// A command started because the person said yes.
    Confirmed {
        /// The command line.
        line: String,
    },
    /// A command started because a standing grant stood in for the ask.
    GrantUsed {
        /// The grant.
        grant: StandingGrantId,
        /// The command line.
        line: String,
    },
    /// The person's "always" became a standing grant.
    GrantStored {
        /// The grant.
        grant: StandingGrantId,
    },
    /// A command did not start.
    Refused {
        /// The command line.
        line: String,
    },
}
