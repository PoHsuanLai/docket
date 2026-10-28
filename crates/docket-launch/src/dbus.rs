//! The real porter link: the accounts service's launcher interface (`porter_client::Launcher`) and its
//! `org.quire.Inference1.Agents` (`OpenEndpoint`, `CloseEndpoint`) over the bus. Built only with
//! the `dbus` feature. A secret passes through here once, from the bus reply into the caller's
//! hands, and is in no log: `SecretText`'s `Debug` shows nothing, and no error carries a value.

use crate::accounts::{
    AccountFault, Accounts, AskKind, Heard, Issued, LoginAsk, OpenedEndpoint, RouteWish,
};
use crate::config::EndpointKind;
use futures_util::StreamExt;
use porter_capability::LoginOutcome;
use porter_capability::{AgentProgram, AgentProtocol, EnvName};
use porter_client::{CallFailure, ChildKey, Delivery as KeyDelivery, Launcher, LauncherError};
use porter_client::{Requests, Revocations};
use porter_core::{DataClass, GrantId, ProcessCredentialId, SecretText, SessionToken};
use porter_dbus::{
    BusConnection,
    accounts::LauncherFault,
    inference::{AgentsProxy, MODELS_ANY, MODELS_LISTED, ROUTE_ACCOUNT, ROUTE_MODEL},
};
use std::collections::HashMap;
use tokio::sync::Mutex;

/// The launcher's link to accountd and inferd.
pub struct DbusAccounts {
    launcher: Launcher,
    agents: AgentsProxy<'static>,
    requests: Mutex<Requests>,
    revocations: Mutex<Revocations>,
}

impl std::fmt::Debug for DbusAccounts {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbusAccounts").finish_non_exhaustive()
    }
}

fn fault(error: &LauncherError) -> AccountFault {
    match error {
        LauncherError::Bus(CallFailure::Unreachable) => AccountFault::Unreachable,
        LauncherError::Refused(_)
        | LauncherError::Bus(CallFailure::Denied(_))
        | LauncherError::Fault(LauncherFault::OnceGrant) => AccountFault::Refused,
        _ => AccountFault::Other,
    }
}

fn bus_fault(error: &zbus::Error) -> AccountFault {
    match error {
        zbus::Error::MethodError(..) => AccountFault::Refused,
        zbus::Error::InputOutput(_) => AccountFault::Unreachable,
        _ => AccountFault::Other,
    }
}

fn slug<T: serde::Serialize>(value: &T) -> Result<String, AccountFault> {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => Ok(s),
        _ => Err(AccountFault::Other),
    }
}

impl DbusAccounts {
    /// Connects over `connection`, which accountd must know as the agent launcher. The request
    /// and revocation streams are subscribed before anything is registered or issued, so none
    /// is missed.
    pub async fn connect(connection: &BusConnection) -> Result<Self, AccountFault> {
        let launcher = Launcher::connect(connection).await.map_err(|e| fault(&e))?;
        let requests = launcher.requests().await.map_err(|e| fault(&e))?;
        let revocations = launcher.revocations().await.map_err(|e| fault(&e))?;
        let agents = AgentsProxy::new(connection)
            .await
            .map_err(|e| bus_fault(&e))?;
        Ok(Self {
            launcher,
            agents,
            requests: Mutex::new(requests),
            revocations: Mutex::new(revocations),
        })
    }
}

impl Accounts for DbusAccounts {
    async fn register(&self, programs: &[AgentProgram]) -> Result<(), AccountFault> {
        self.launcher
            .register(programs)
            .await
            .map_err(|e| fault(&e))
    }

    async fn begin_session(&self, session: &SessionToken) -> Result<(), AccountFault> {
        self.launcher
            .begin_session(session)
            .await
            .map_err(|e| fault(&e))
    }

    async fn end_session(&self, session: &SessionToken) -> Result<(), AccountFault> {
        self.launcher
            .end_session(session)
            .await
            .map_err(|e| fault(&e))
    }

    async fn open_endpoint(
        &self,
        program: &AgentProgram,
        route: &RouteWish,
        class: DataClass,
        protocol: AgentProtocol,
    ) -> Result<OpenedEndpoint, AccountFault> {
        let kind = match route.kind {
            EndpointKind::Model => ROUTE_MODEL,
            EndpointKind::Account => ROUTE_ACCOUNT,
        };
        let models = if route.models.is_empty() {
            MODELS_ANY
        } else {
            MODELS_LISTED
        };
        let arg = (
            kind.to_owned(),
            route.id.clone(),
            models.to_owned(),
            route.models.clone(),
        );
        let class = slug(&class)?;
        let protocol = slug(&protocol)?;
        let (session, _scheme, host, port, base_url, token, _protocol) = self
            .agents
            .open_endpoint(
                program.as_str(),
                &arg,
                &class,
                &[protocol.as_str()],
                &HashMap::new(),
            )
            .await
            .map_err(|e| bus_fault(&e))?;
        Ok(OpenedEndpoint {
            session,
            host,
            port,
            base_url,
            token: SecretText::new(token),
        })
    }

    async fn close_endpoint(&self, session: &str) -> Result<(), AccountFault> {
        self.agents
            .close_endpoint(session)
            .await
            .map_err(|e| bus_fault(&e))
    }

    async fn request_key(
        &self,
        program: &AgentProgram,
        class: DataClass,
        session: &SessionToken,
    ) -> Result<GrantId, AccountFault> {
        self.launcher
            .request_agent_key(program, class, Some(session))
            .await
            .map(|candidate| candidate.permit)
            .map_err(|e| fault(&e))
    }

    async fn issue(
        &self,
        permit: &GrantId,
        program: &AgentProgram,
        key_env: &EnvName,
    ) -> Result<Issued, AccountFault> {
        let credential = self
            .launcher
            .issue_credential(permit, program)
            .await
            .map_err(|e| fault(&e))?;
        let ChildKey::Value { value: key, .. } = credential
            .child_key(key_env, KeyDelivery::Value)
            .map_err(|_| AccountFault::Other)?
        else {
            return Err(AccountFault::Other);
        };
        Ok(Issued {
            id: credential.id().clone(),
            key,
        })
    }

    async fn revoke(&self, id: &ProcessCredentialId) -> Result<(), AccountFault> {
        self.launcher
            .revoke_credential(id)
            .await
            .map_err(|e| fault(&e))
    }

    async fn hear(&self) -> Option<Heard> {
        let mut requests = self.requests.lock().await;
        let mut revocations = self.revocations.lock().await;
        loop {
            // A message that is not well formed is not answered; the next is heard.
            tokio::select! {
                ask = requests.next() => {
                    if let Ok(ask) = ask? {
                        return Some(Heard::Ask(LoginAsk {
                            kind: match ask.kind {
                                porter_client::AskKind::Login => AskKind::Login,
                                porter_client::AskKind::Logout => AskKind::Logout,
                            },
                            request: ask.request,
                            account: ask.account,
                            program: ask.program,
                        }));
                    }
                }
                gone = revocations.next() => {
                    if let Ok(revoked) = gone? {
                        return Some(Heard::Revoked(revoked.id));
                    }
                }
            }
        }
    }

    async fn report(&self, ask: &LoginAsk, outcome: LoginOutcome) -> Result<(), AccountFault> {
        let result = match ask.kind {
            AskKind::Login => self.launcher.report_login(&ask.request, outcome).await,
            AskKind::Logout => self.launcher.report_logout(&ask.request, outcome).await,
        };
        result.map_err(|e| fault(&e))
    }
}
