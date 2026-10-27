//! Signing out an agent that has no sign-out of its own: the logout command of its entry in
//! `agents.toml`, run where the person can see it, outside the agent's sandbox, with its output
//! read by no one. An entry with no logout command cannot be signed out, and says so.

use super::live::Live;
use docket_acp_host::Logout;
use docket_core::ProgramName;
use docket_core::helper::AgentPending;
use docket_launch::login::{LoginRunner, VisibleLogin};
use docket_launch::{Accounts, AskKind};
use porter_core::LoginOutcome;
use prov::AgentProgram;
use std::sync::Arc;

/// Runs the logout command of an agent's entry on a live host.
pub struct EntryLogout<A: Accounts + 'static> {
    live: Arc<Live<A>>,
    runner: VisibleLogin,
}

impl<A: Accounts + 'static> std::fmt::Debug for EntryLogout<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryLogout").finish_non_exhaustive()
    }
}

impl<A: Accounts + 'static> EntryLogout<A> {
    /// The logout commands of the entries `live` holds, run by `runner`.
    pub fn new(live: Arc<Live<A>>, runner: VisibleLogin) -> Self {
        Self { live, runner }
    }
}

impl<A: Accounts + 'static> Logout for EntryLogout<A> {
    fn run<'a>(&'a self, program: &'a AgentProgram) -> AgentPending<'a, bool> {
        Box::pin(async move {
            let Some(entry) = self.live.entry(&ProgramName::from(program)) else {
                return false;
            };
            if entry.logout.is_empty() {
                return false;
            }
            matches!(
                self.runner.run(entry, AskKind::Logout).await,
                LoginOutcome::Ready
            )
        })
    }
}
