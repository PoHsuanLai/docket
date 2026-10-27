//! Where an agent's start went wrong. `BackendFault::Unavailable` is one fault for many causes (a
//! folder the sandbox may not write, a process that did not start, an agent that answered
//! nonsense); the person is told one sentence, so the cause is written to the log at the step
//! where it is dropped, and [`Step`] names that step.

use docket_session::BackendFault;
use std::fmt;

/// The step of starting an agent that failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Step {
    /// The opening names no agent program or no folder.
    Opening,
    /// The router did not open the agent's session.
    Router,
    /// The folder the agent works in was refused.
    Folder,
    /// The agent process did not start.
    Spawn,
    /// The folder could not be resolved to the real one.
    Resolve,
    /// A message could not be sent to the agent.
    Write,
    /// The agent closed its output or answered with an error.
    Reply,
    /// The agent's answer to `initialize` was not usable.
    Initialize,
    /// The agent's answer to `session/new` was not usable.
    NewSession,
    /// The agent did not finish the handshake in time.
    Timeout,
}

impl Step {
    /// The step as a clause of the log line.
    pub(super) fn words(self) -> &'static str {
        match self {
            Step::Opening => "the opening names no agent or folder",
            Step::Router => "the router did not open the session",
            Step::Folder => "the working folder was refused",
            Step::Spawn => "the agent did not start",
            Step::Resolve => "the working folder could not be resolved",
            Step::Write => "the agent could not be written to",
            Step::Reply => "the agent closed or refused",
            Step::Initialize => "the agent's start-up answer was not usable",
            Step::NewSession => "the agent's new-session answer was not usable",
            Step::Timeout => "the agent did not answer in time",
        }
    }

    /// The log line for this step failing because of `cause`.
    pub(super) fn told(self, cause: &dyn fmt::Display) -> String {
        format!("{}: {cause}", self.words())
    }

    /// `Unavailable`, with the step and its cause written to the log.
    pub(super) fn unavailable(self, cause: &dyn fmt::Display) -> BackendFault {
        tracing::warn!("{}", self.told(cause));
        BackendFault::Unavailable
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::{SandboxStep, SpawnFault};

    #[test]
    fn a_failed_start_keeps_its_step_and_its_cause() {
        let cause = SpawnFault::Sandbox(SandboxStep::Forwarder);
        let told = Step::Spawn.told(&cause);
        assert!(told.contains("the agent did not start"), "{told}");
        assert!(told.contains("network forwarder"), "{told}");
        let other = Step::Spawn.told(&SpawnFault::Sandbox(SandboxStep::Start));
        assert_ne!(told, other);
        assert_eq!(Step::Spawn.unavailable(&cause), BackendFault::Unavailable);
    }

    #[test]
    fn every_step_has_its_own_words() {
        let steps = [
            Step::Opening,
            Step::Router,
            Step::Folder,
            Step::Spawn,
            Step::Resolve,
            Step::Write,
            Step::Reply,
            Step::Initialize,
            Step::NewSession,
            Step::Timeout,
        ];
        let words: std::collections::BTreeSet<_> = steps.iter().map(|s| s.words()).collect();
        assert_eq!(words.len(), steps.len());
    }
}
