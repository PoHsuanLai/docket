//! The agent apps that are helpers, hosted here: one agent process per helper
//! (`docket_acp_host::HelperAgents`), each opened at the router as this process's role, which
//! is the only role that may open an outside agent's session. companiond keeps the board and
//! asks for a helper's job, sign-in, check, sign-out or removal through `AgentHost1`
//! (`AppsBegin`, `AppsWait`); a fault in one helper's agent ends that helper's job and no other.
//!
//! An app that cannot run says so in plain words: not in the list of coding agents, in it but not
//! installed, or coding agents turned off. Which program an app is, and where its command is,
//! come from `agents.toml` (the person's list, written from what Settings found on the `PATH`);
//! nothing here knows a path.

use super::brought_hosts::BroughtHosts;
use super::fault::AgentFault;
use super::helper_hosts::{HelperHosts, folder_of};
use super::live::Live;
use super::logout::EntryLogout;
use docket_acp_host::{HelperAgent, HelperAgents};
use docket_core::helper::{
    AgentApps, AgentOutcome, AppsAsk, AppsReply, BackOutcome, BroughtOutcome, CheckOutcome,
    HandFault, HelperCard, JobFault, SignInOutcome, SignOutOutcome, TerminalHand, TerminalLogin,
    ToTerminalOutcome,
};
use docket_core::{SeenCommand, WatchedAgent};
use docket_launch::dbus::DbusAccounts;
use docket_permissions_core::ProgramName;
use docket_vocab::Workspace;

use docket_store::SystemClock;
use docket_vocab::Clock;
use prov::SessionId;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::Mutex as AsyncMutex;

type Maker = Box<dyn Fn(&HelperCard) -> HelperHosts<DbusAccounts> + Send + Sync>;

/// An agent app a terminal started that the person brought into docket.
type Brought = Arc<AsyncMutex<HelperAgent<BroughtHosts<DbusAccounts>>>>;

/// The helpers' agent apps, running.
pub struct HelperHost {
    live: Arc<Live<DbusAccounts>>,
    agents: HelperAgents<HelperHosts<DbusAccounts>, Maker>,
    brought: Mutex<BTreeMap<SessionId, Brought>>,
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
        let logout = Arc::new(EntryLogout::new(
            Arc::clone(&live),
            live.places().login_runner(),
        ));
        let agents = HelperAgents::new(make, || SystemClock.now()).with_logout(logout);
        Self {
            live,
            agents,
            brought: Mutex::new(BTreeMap::new()),
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
        let all = std::mem::take(&mut *self.brought.lock().unwrap_or_else(|e| e.into_inner()));
        for agent in all.into_values() {
            agent.lock().await.close().await;
        }
    }

    /// The helper's conversation, opened in the app's own terminal program: its command is built
    /// before anything is closed, and the app is brought back if it cannot be built after.
    async fn to_terminal(&self, card: &HelperCard) -> ToTerminalOutcome {
        let refused = ToTerminalOutcome::Refused;
        let (Ok(()), Some(program)) = (self.ready(card), card.kind.program()) else {
            return refused(HandFault::Failed);
        };
        let program = ProgramName::from(program);
        let folder = folder_of(&card.id, self.live.places());
        // A line that cannot be built stops here, with the app still open.
        if self
            .live
            .resume_run(&program, "check", folder.as_deref())
            .is_none()
        {
            return refused(HandFault::NotResumable);
        }
        let away = match self.agents.to_terminal(card).await {
            Ok(away) => away,
            Err(why) => return refused(why),
        };
        match self
            .live
            .resume_run(&program, away.agent.session.as_str(), folder.as_deref())
        {
            Some(run) => ToTerminalOutcome::Ready(Box::new(TerminalHand {
                run,
                session: away.session,
            })),
            None => {
                let _ = self.agents.bring_back(card).await;
                refused(HandFault::Failed)
            }
        }
    }

    async fn bring_back(&self, card: &HelperCard) -> BackOutcome {
        match self.ready(card) {
            Ok(()) => match self.agents.bring_back(card).await {
                Ok(()) => BackOutcome::Back,
                Err(why) => BackOutcome::Refused(why),
            },
            Err(_) => BackOutcome::Refused(HandFault::Failed),
        }
    }

    /// Starts `agent` on the watch session `session` a terminal opened, in the folder the
    /// terminal ran in, as a conversation of docket's.
    async fn bring_back_watched(
        &self,
        session: SessionId,
        agent: WatchedAgent,
        folder: Workspace,
    ) -> BroughtOutcome {
        let refused = BroughtOutcome::Refused;
        let Some(entry) = self.live.entry(&agent.program) else {
            return refused(HandFault::NotResumable);
        };
        if !Path::new(entry.command.as_str()).exists() {
            return refused(HandFault::Failed);
        }
        let hosts = BroughtHosts::new(
            Arc::clone(&self.live),
            agent.program.clone(),
            &session,
            folder,
        );
        let mut conversation = HelperAgent::new(hosts).already_away(session.clone(), agent);
        match conversation.bring_back().await {
            Ok(()) => {
                let kept = Arc::new(AsyncMutex::new(conversation));
                self.brought
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(session.clone(), kept);
                BroughtOutcome::Brought(session)
            }
            Err(why) => refused(why),
        }
    }

    /// The app and conversation a terminal's command line picks up, by the live agents list.
    fn recognise(&self, seen: &SeenCommand) -> Option<WatchedAgent> {
        self.live.recognise(seen)
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
        folder_of(&card.id, self.live.places())
            .map(|_| ())
            .ok_or(JobFault::Crashed)
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
                folder_of(&card.id, self.live.places()).as_deref(),
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
            AppsAsk::ToTerminal { card } => AppsReply::ToTerminal(self.to_terminal(&card).await),
            AppsAsk::BringBack { card } => AppsReply::BringBack(self.bring_back(&card).await),
            AppsAsk::BringBackWatched {
                session,
                agent,
                folder,
            } => AppsReply::BringBackWatched(self.bring_back_watched(session, agent, folder).await),
            AppsAsk::Recognise { seen } => AppsReply::Recognise(self.recognise(&seen)),
        }
    }

    /// The reply to `ask` when the host could not start: the setting is off, the list is missing
    /// or there is no sandbox (each told apart). A call that only ends something has nothing to end.
    pub fn unavailable(ask: &AppsAsk, fault: AgentFault) -> AppsReply {
        // The person is told one sentence; the cause is kept here, where it would be dropped.
        eprintln!("docket-agent: the helpers' agents cannot be hosted: {fault}");
        Self::failed(ask, Self::told(&fault))
    }

    /// What the person is told of a host that could not start.
    fn told(fault: &AgentFault) -> JobFault {
        match fault {
            AgentFault::Permit(_) => JobFault::TurnedOff,
            AgentFault::NoAgentsFile | AgentFault::Agents(_) => JobFault::NotSetUp,
            AgentFault::NoSandbox(_) => JobFault::NoSandbox,
            _ => JobFault::Crashed,
        }
    }

    fn failed(ask: &AppsAsk, why: JobFault) -> AppsReply {
        match ask {
            AppsAsk::Run { .. } => AppsReply::Run(AgentOutcome::Failed(why)),
            AppsAsk::Stop { .. } => AppsReply::Stop,
            AppsAsk::SignIn { .. } => AppsReply::SignIn(SignInOutcome::Failed),
            AppsAsk::Check { .. } => AppsReply::Check(CheckOutcome::Failed),
            AppsAsk::SignOut { .. } => AppsReply::SignOut(SignOutOutcome::Failed),
            AppsAsk::Forget { .. } => AppsReply::Forget,
            AppsAsk::ToTerminal { .. } => {
                AppsReply::ToTerminal(ToTerminalOutcome::Refused(HandFault::Failed))
            }
            AppsAsk::BringBack { .. } => {
                AppsReply::BringBack(BackOutcome::Refused(HandFault::Failed))
            }
            AppsAsk::BringBackWatched { .. } => {
                AppsReply::BringBackWatched(BroughtOutcome::Refused(HandFault::Failed))
            }
            AppsAsk::Recognise { .. } => AppsReply::Recognise(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_sandbox_is_told_as_its_own_sentence() {
        let none = AgentFault::NoSandbox(bulkhead::CannotSandbox::NotInstalled);
        let why = HelperHost::told(&none);
        assert_eq!(why, JobFault::NoSandbox);
        assert!(!why.plain().contains("crash") && !why.plain().contains("bubblewrap"));
        assert_ne!(why.plain(), JobFault::Crashed.plain());
    }
}
