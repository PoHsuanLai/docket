//! What one ask came to.

use docket_client::ClientError;
use docket_core::StepLine;
use docket_tasks::Failure;

/// How an ask ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// The agent finished.
    Done,
    /// The agent could not go on; [`Run::failure`] says why when it knows.
    Failed,
    /// The router halted it, or it was cancelled.
    Cancelled,
    /// The agent asked a question and waits for an answer; the ask is over and a new one may
    /// carry the answer.
    Asked {
        /// The question.
        text: String,
        /// The choices it offered, if any.
        choices: Vec<String>,
    },
    /// The breaker paused it after repeated refusals.
    Paused {
        /// What the agent says about it.
        text: String,
    },
}

/// The result of [`crate::Agent::ask`]. Nothing in it is the content of an untrusted source: the
/// steps are the planner's own view of them (handles and coarse refusals).
#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    /// How it ended.
    pub ended: Ended,
    /// What the agent said, in order.
    pub said: Vec<String>,
    /// The calls it made and how each ended, oldest first. A refusal is the router's coarse
    /// code (`CallRefusal`), never a policy id or a reviewer's words.
    pub steps: Vec<StepLine>,
    /// Why it failed, when a failure is known.
    pub failure: Option<Failure>,
    /// Why the router did not close the session after the ask, when it did not. The ask ran
    /// all the same; the session is left to the router's own expiry.
    pub close: Option<ClientError>,
}
