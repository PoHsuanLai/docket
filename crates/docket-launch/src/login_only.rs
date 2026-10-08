//! `LoginOnly`: the account seam for a launcher with no porter behind it. Only the `login` route
//! can work: the agent signs itself in and nothing is lent. Every route that needs a key or an
//! endpoint is refused, so an entry that asks for one fails to start instead of running without
//! what it was promised. It is what the live-eval harness hosts an agent with on its private bus,
//! where there is no accountd.

use crate::accounts::{AccountFault, Accounts, Heard, Issued, LoginAsk, OpenedEndpoint, RouteWish};
use crate::config::Delivery;
use porter_core::capability::{AgentProgram, AgentProtocol, EnvName};
use porter_core::{DataClass, GrantId, LauncherSession, LoginOutcome, ProcessCredentialId};

/// The seam with nothing behind it.
#[derive(Debug, Clone, Copy, Default)]
pub struct LoginOnly;

impl Accounts for LoginOnly {
    async fn register(&self, _programs: &[AgentProgram]) -> Result<(), AccountFault> {
        Ok(())
    }

    async fn begin_session(&self, _session: &LauncherSession) -> Result<(), AccountFault> {
        Ok(())
    }

    async fn end_session(&self, _session: &LauncherSession) -> Result<(), AccountFault> {
        Ok(())
    }

    async fn open_endpoint(
        &self,
        _program: &AgentProgram,
        _route: &RouteWish,
        _class: DataClass,
        _protocol: AgentProtocol,
    ) -> Result<OpenedEndpoint, AccountFault> {
        Err(AccountFault::Unreachable)
    }

    async fn close_endpoint(&self, _session: &str) -> Result<(), AccountFault> {
        Ok(())
    }

    async fn request_grant(
        &self,
        _program: &AgentProgram,
        _class: DataClass,
        _session: &LauncherSession,
    ) -> Result<GrantId, AccountFault> {
        Err(AccountFault::Unreachable)
    }

    async fn issue(
        &self,
        _grant: &GrantId,
        _program: &AgentProgram,
        _key_env: &EnvName,
        _delivery: Delivery,
    ) -> Result<Issued, AccountFault> {
        Err(AccountFault::Unreachable)
    }

    async fn revoke(&self, _id: &ProcessCredentialId) -> Result<(), AccountFault> {
        Ok(())
    }

    async fn hear(&self) -> Option<Heard> {
        None
    }

    async fn report(&self, _ask: &LoginAsk, _outcome: LoginOutcome) -> Result<(), AccountFault> {
        Ok(())
    }
}
