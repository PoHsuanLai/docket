//! The real porter link: accountd's launcher interface (`porter_client::Launcher`) and inferd's
//! `org.quire.Inference1.Agents` (`OpenEndpoint`, `CloseEndpoint`) over the bus. Built only with
//! the `dbus` feature. A secret passes through here once, from the bus reply into the caller's
//! hands, and is in no log: `SecretText`'s `Debug` shows nothing, and no error carries a value.

use crate::accounts::{
    AccountFault, Accounts, AskKind, Heard, Issued, KeyHandoff, LoginAsk, OpenedEndpoint, RouteWish,
};
use crate::config::Delivery;
use docket_core::AbsPath;
use futures_util::StreamExt;
use porter_client::{ChildKey, Delivery as KeyDelivery, Launcher, LauncherError};
use porter_client::{Requests, Revocations};
use porter_core::audit::Handoff;
use porter_core::capability::{AgentProgram, AgentProtocol, EnvName};
use porter_core::{
    DataClass, GrantId, LauncherSession, LoginOutcome, ProcessCredentialId, SecretText,
};
use porter_dbus::{
    AgentsProxy, BusConnection, MODELS_ANY, MODELS_LISTED, ROUTE_ACCOUNT, ROUTE_MODEL,
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
        LauncherError::Unreachable => AccountFault::Unreachable,
        LauncherError::Refused(_) | LauncherError::Denied(_) | LauncherError::OnceGrant => {
            AccountFault::Refused
        }
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

    async fn begin_session(&self, session: &LauncherSession) -> Result<(), AccountFault> {
        self.launcher
            .begin_session(session)
            .await
            .map_err(|e| fault(&e))
    }

    async fn end_session(&self, session: &LauncherSession) -> Result<(), AccountFault> {
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
        let kind = if route.kind == "model" {
            ROUTE_MODEL
        } else {
            ROUTE_ACCOUNT
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

    async fn request_grant(
        &self,
        program: &AgentProgram,
        class: DataClass,
        session: &LauncherSession,
    ) -> Result<GrantId, AccountFault> {
        self.launcher
            .request_agent_grant(program, class, Some(session))
            .await
            .map(|candidate| candidate.grant)
            .map_err(|e| fault(&e))
    }

    async fn issue(
        &self,
        grant: &GrantId,
        program: &AgentProgram,
        key_env: &EnvName,
        delivery: Delivery,
    ) -> Result<Issued, AccountFault> {
        let (handoff, how) = match delivery {
            Delivery::Value => (Handoff::Memfd, KeyDelivery::Value),
            // A tmpfs file has no descriptor to pass; the number is ignored.
            Delivery::File => (Handoff::TmpfsFile, KeyDelivery::File { child_fd: 0 }),
        };
        let credential = self
            .launcher
            .issue_credential(grant, program, handoff)
            .await
            .map_err(|e| fault(&e))?;
        let key = match credential
            .child_key(key_env, how)
            .map_err(|_| AccountFault::Other)?
        {
            ChildKey::Value { value, .. } => KeyHandoff::Value(value),
            ChildKey::File { path, .. } => {
                KeyHandoff::File(AbsPath::parse(&path).map_err(|_| AccountFault::Other)?)
            }
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
