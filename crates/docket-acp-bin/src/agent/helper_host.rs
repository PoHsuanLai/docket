//! The agent apps that are helpers, hosted here: one agent process per helper
//! (`docket_acp::client::HelperAgents`), each opened at the router as this process's role, which
//! is the only role that may open an outside agent's session. companiond keeps the board and
//! asks for a helper's job, sign-in, check, sign-out or removal through `AgentHost1`
//! (`AppsBegin`, `AppsWait`); a fault in one helper's agent ends that helper's job and no other.
//!
//! An app that cannot run says so in plain words: not in the list of coding agents, in it but not
//! installed, or coding agents turned off. Which program an app is, and where its command is,
//! come from `agents.toml` (the person's list, written from what Settings found on the `PATH`);
//! nothing here knows a path.

use super::fault::AgentFault;
use super::helper_hosts::{HelperHosts, folder_of};
use super::live::Live;
use super::logout::EntryLogout;
use super::wire;
use docket_acp::client::HelperAgents;
use docket_acp::{SystemTicks, Ticks};
use docket_core::ProgramName;
use docket_core::helper::{
    AgentApps, AgentOutcome, AppsAsk, AppsReply, CheckOutcome, HelperCard, JobFault, SignInOutcome,
    SignOutOutcome, TerminalLogin,
};
use docket_launch::dbus::DbusAccounts;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

type Maker = Box<dyn Fn(&HelperCard) -> HelperHosts<DbusAccounts> + Send + Sync>;

/// The helpers' agent apps, running.
pub struct HelperHost {
    live: Arc<Live<DbusAccounts>>,
    agents: HelperAgents<HelperHosts<DbusAccounts>, Maker>,
    running: AtomicUsize,
}

impl std::fmt::Debug for HelperHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperHost")
    }
}

/// Counts a call while it runs, so the host does not end under it.
struct Counted<'a>(&'a AtomicUsize);

impl<'a> Counted<'a> {
    fn start(count: &'a AtomicUsize) -> Self {
        count.fetch_add(1, Ordering::SeqCst);
        Self(count)
    }
}

impl Drop for Counted<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

impl HelperHost {
    /// Gets ready to start agents when a helper needs one, over the pseudo-app `live` serves for
    /// every use of this process.
    pub fn new(live: Arc<Live<DbusAccounts>>) -> Self {
        let hosts = Arc::clone(&live);
        let make: Maker = Box::new(move |card| HelperHosts::new(Arc::clone(&hosts), card));
        let logout = Arc::new(EntryLogout::new(Arc::clone(&live), wire::login_runner()));
        let agents = HelperAgents::new(make, || SystemTicks.now()).with_logout(logout);
        Self {
            live,
            agents,
            running: AtomicUsize::new(0),
        }
    }

    /// Whether a call is running.
    pub fn busy(&self) -> bool {
        self.running.load(Ordering::SeqCst) > 0
    }

    /// Ends every helper's agent process, on the way out.
    pub async fn shutdown(&self) {
        self.agents.shutdown().await;
    }

    /// Why the helper's app cannot start, or nothing if it can.
    fn ready(&self, card: &HelperCard) -> Result<(), JobFault> {
        let program = card.kind.program().ok_or(JobFault::Crashed)?;
        let entry = self
            .live
            .entry(&ProgramName::from(program))
            .ok_or(JobFault::NotSetUp)?;
        if !Path::new(entry.command.as_str()).exists() {
            return Err(JobFault::NotInstalled);
        }
        folder_of(&card.id).map(|_| ()).ok_or(JobFault::Crashed)
    }

    /// A sign-in that is to run in a terminal, with the command resolved from the list of coding
    /// agents and put in the sandbox. If that cannot be done the sign-in fails: the person is never
    /// sent to run the agent unconfined, or to guess its command.
    fn resolved(&self, card: &HelperCard, outcome: SignInOutcome) -> SignInOutcome {
        let SignInOutcome::OpenTerminal(login) = outcome else {
            return outcome;
        };
        let run = card.kind.program().and_then(|program| {
            self.live.terminal_run(
                &ProgramName::from(program),
                &login,
                folder_of(&card.id).as_deref(),
            )
        });
        match run {
            Some(run) => SignInOutcome::OpenTerminal(TerminalLogin {
                run: Some(Box::new(run)),
                ..login
            }),
            None => SignInOutcome::Failed,
        }
    }

    /// Carries out one call on the helpers' agents.
    pub async fn ask(&self, ask: AppsAsk) -> AppsReply {
        let _counted = Counted::start(&self.running);
        match ask {
            AppsAsk::Run { card, job } => match self.ready(&card) {
                Ok(()) => AppsReply::Run(self.agents.run(&card, &job).await),
                Err(why) => AppsReply::Run(AgentOutcome::Failed(why)),
            },
            AppsAsk::Stop { helper } => {
                self.agents.stop(&helper).await;
                AppsReply::Stop
            }
            AppsAsk::SignIn { card, way } => match self.ready(&card) {
                Ok(()) => {
                    let outcome = self.agents.sign_in(&card, &way).await;
                    AppsReply::SignIn(self.resolved(&card, outcome))
                }
                Err(_) => AppsReply::SignIn(SignInOutcome::Failed),
            },
            AppsAsk::Check { card } => match self.ready(&card) {
                Ok(()) => AppsReply::Check(self.agents.check(&card).await),
                Err(_) => AppsReply::Check(CheckOutcome::Failed),
            },
            AppsAsk::SignOut { card } => match self.ready(&card) {
                Ok(()) => AppsReply::SignOut(self.agents.sign_out(&card).await),
                Err(_) => AppsReply::SignOut(SignOutOutcome::Failed),
            },
            AppsAsk::Forget { helper } => {
                self.agents.forget(&helper).await;
                AppsReply::Forget
            }
        }
    }

    /// The reply to `ask` when the host could not start: the setting is off, the list is missing
    /// or there is no sandbox. A call that only ends something has nothing to end.
    pub fn unavailable(ask: &AppsAsk, fault: AgentFault) -> AppsReply {
        let why = match fault {
            AgentFault::Permit(_) => JobFault::TurnedOff,
            AgentFault::NoAgentsFile | AgentFault::Agents(_) => JobFault::NotSetUp,
            _ => JobFault::Crashed,
        };
        Self::failed(ask, why)
    }

    fn failed(ask: &AppsAsk, why: JobFault) -> AppsReply {
        match ask {
            AppsAsk::Run { .. } => AppsReply::Run(AgentOutcome::Failed(why)),
            AppsAsk::Stop { .. } => AppsReply::Stop,
            AppsAsk::SignIn { .. } => AppsReply::SignIn(SignInOutcome::Failed),
            AppsAsk::Check { .. } => AppsReply::Check(CheckOutcome::Failed),
            AppsAsk::SignOut { .. } => AppsReply::SignOut(SignOutOutcome::Failed),
            AppsAsk::Forget { .. } => AppsReply::Forget,
        }
    }
}
