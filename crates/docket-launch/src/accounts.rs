//! What a launcher asks of porter, behind a seam: accountd (sessions, grants, process
//! credentials, the agent's login) and inferd (the per-session model endpoint, P4). The real link
//! is `dbus::DbusAccounts`; `fake::FakeAccounts` stands in for it in tests. Methods take `&self`
//! because one link serves every session and the supervisor listens on it at the same time.
//!
//! The types are ours so this file names no bus: a secret is porter's `SecretText` (its `Debug`
//! shows nothing).

use crate::config::Delivery;
use docket_core::AbsPath;
use porter_core::capability::{AgentProgram, AgentProtocol};
use porter_core::{
    AccountId, DataClass, GrantId, LauncherSession, LoginOutcome, LoginRequestId,
    ProcessCredentialId, SecretText,
};
use std::future::Future;

/// Why a call to porter failed. Coarse on purpose: nothing of the other side's wording is kept.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AccountFault {
    /// The service is not there.
    #[error("the account service is not reachable")]
    Unreachable,
    /// It refused: the person said no, or this caller may not.
    #[error("the account service refused")]
    Refused,
    /// Anything else.
    #[error("the account service failed")]
    Other,
}

/// What the endpoint serves, as the bus names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteWish {
    /// `account` or `model`.
    pub kind: &'static str,
    /// The account id or catalogue model id.
    pub id: String,
    /// Model ids the agent may name; empty means any.
    pub models: Vec<String>,
}

/// An endpoint inferd opened for one session.
#[derive(Debug, Clone)]
pub struct OpenedEndpoint {
    /// inferd's own name for the session (the one to close).
    pub session: String,
    /// The host: `127.0.0.1`.
    pub host: String,
    /// The port.
    pub port: u16,
    /// The base URL to give the agent.
    pub base_url: String,
    /// What the agent sends as its API key. A per-session token, not the account's key.
    pub token: SecretText,
}

/// A key handed to one process.
#[derive(Debug)]
pub struct Issued {
    /// To revoke it by, and what a revocation names.
    pub id: ProcessCredentialId,
    /// How the child gets it.
    pub key: KeyHandoff,
}

/// The key as the child is to receive it.
#[derive(Debug)]
pub enum KeyHandoff {
    /// The text, for the child's environment.
    Value(SecretText),
    /// A file on a tmpfs, for `<key_env>_FILE`.
    File(AbsPath),
}

/// A person's request to sign an agent in or out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginAsk {
    /// In or out.
    pub kind: AskKind,
    /// To report under.
    pub request: LoginRequestId,
    /// The account the request is for.
    pub account: AccountId,
    /// The program.
    pub program: AgentProgram,
}

/// In or out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AskKind {
    /// Sign in.
    Login,
    /// Sign out.
    Logout,
}

/// What porter tells a launcher without being asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Heard {
    /// Run the agent's own login or logout.
    Ask(LoginAsk),
    /// accountd ended this credential: the process that holds it must end.
    Revoked(ProcessCredentialId),
}

/// The seam.
pub trait Accounts: Send + Sync {
    /// Registers the programs this launcher runs, for as long as it lives.
    fn register(
        &self,
        programs: &[AgentProgram],
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;

    /// Opens a launcher session (the span a "this session only" grant lasts).
    fn begin_session(
        &self,
        session: &LauncherSession,
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;

    /// Ends it: accountd removes its grants and ends the credentials under them.
    fn end_session(
        &self,
        session: &LauncherSession,
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;

    /// inferd opens a loopback endpoint for `program` over `route` (P4).
    fn open_endpoint(
        &self,
        program: &AgentProgram,
        route: &RouteWish,
        class: DataClass,
        protocol: AgentProtocol,
    ) -> impl Future<Output = Result<OpenedEndpoint, AccountFault>> + Send;

    /// Closes it.
    fn close_endpoint(
        &self,
        session: &str,
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;

    /// Asks the person which account `program` may use for this session (P2).
    fn request_grant(
        &self,
        program: &AgentProgram,
        class: DataClass,
        session: &LauncherSession,
    ) -> impl Future<Output = Result<GrantId, AccountFault>> + Send;

    /// A key for one process under `grant`, handed over as `delivery` says.
    fn issue(
        &self,
        grant: &GrantId,
        program: &AgentProgram,
        key_env: &porter_core::capability::EnvName,
        delivery: Delivery,
    ) -> impl Future<Output = Result<Issued, AccountFault>> + Send;

    /// Ends a credential because its process ended.
    fn revoke(
        &self,
        id: &ProcessCredentialId,
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;

    /// The next thing porter says to this launcher; `None` when the link is gone.
    fn hear(&self) -> impl Future<Output = Option<Heard>> + Send;

    /// Reports what became of a login request.
    fn report(
        &self,
        ask: &LoginAsk,
        outcome: LoginOutcome,
    ) -> impl Future<Output = Result<(), AccountFault>> + Send;
}
