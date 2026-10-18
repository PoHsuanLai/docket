//! `HelperAgents`: the host of the agent apps that are helpers. One [`HelperAgent`] per helper,
//! made the first time the helper is used, each with its own process, so a fault in one helper's
//! agent is a fault in that helper's job and nothing else. It is the board's seam to the agent
//! apps (`AgentApps`): a job in, a typed outcome out.

use super::isolate::{Caught, caught};
use super::helper_agent::{AgentEnd, HelperAgent, Hosts, SignInStep, SignOutStep, Trouble};
use docket_core::helper::{
    AgentApps, AgentOutcome, AgentPending, CheckOutcome, HelperCard, Job, JobFault, SignInOutcome,
    SignInWay, SignOutOutcome,
};
use prov::{AgentProgram, HelperId, UnixSeconds};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tokio::sync::{Mutex as AsyncMutex, Notify};

/// The logout command of an agent's entry in the agent list, for an agent that has no sign-out of
/// its own (`docket-launch` runs it, and reports only the outcome).
pub trait Logout: Send + Sync {
    /// Signs `program` out with its own command. True if it did.
    fn run<'a>(&'a self, program: &'a AgentProgram) -> AgentPending<'a, bool>;
}

/// One helper's agent, and how to stop the job it is running.
struct Slot<H: Hosts> {
    agent: Arc<AsyncMutex<HelperAgent<H>>>,
    cancel: Arc<Notify>,
    running: Arc<AtomicBool>,
}

impl<H: Hosts> Clone for Slot<H> {
    fn clone(&self) -> Self {
        Self {
            agent: Arc::clone(&self.agent),
            cancel: Arc::clone(&self.cancel),
            running: Arc::clone(&self.running),
        }
    }
}

/// The agent apps that are helpers: `make` gives the hosts of a helper's agent processes.
pub struct HelperAgents<H: Hosts, F> {
    make: F,
    now: Arc<dyn Fn() -> UnixSeconds + Send + Sync>,
    logout: Option<Arc<dyn Logout>>,
    slots: Mutex<BTreeMap<HelperId, Slot<H>>>,
}

impl<H: Hosts, F> std::fmt::Debug for HelperAgents<H, F> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HelperAgents")
    }
}

fn fault_of(trouble: Trouble) -> JobFault {
    match trouble {
        Trouble::CouldNotStart | Trouble::StoppedAnswering => JobFault::Crashed,
        Trouble::Declined => JobFault::Refused,
    }
}

impl<H, F> HelperAgents<H, F>
where
    H: Hosts + 'static,
    F: Fn(&HelperCard) -> H + Send + Sync,
{
    /// The host of the helpers' agents. `make` builds the hosts of one helper's processes from its
    /// card; `now` is the clock the turns are stamped with.
    pub fn new(make: F, now: impl Fn() -> UnixSeconds + Send + Sync + 'static) -> Self {
        Self {
            make,
            now: Arc::new(now),
            logout: None,
            slots: Mutex::new(BTreeMap::new()),
        }
    }

    /// The same host, signing out an agent that has no sign-out of its own with `logout`.
    #[must_use]
    pub fn with_logout(mut self, logout: Arc<dyn Logout>) -> Self {
        self.logout = Some(logout);
        self
    }

    fn slot(&self, card: &HelperCard) -> Slot<H> {
        let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots
            .entry(card.id.clone())
            .or_insert_with(|| Slot {
                agent: Arc::new(AsyncMutex::new(HelperAgent::new((self.make)(card)))),
                cancel: Arc::new(Notify::new()),
                running: Arc::new(AtomicBool::new(false)),
            })
            .clone()
    }

    /// Runs `work` for `helper`'s agent such that a panic in it is that helper's failure and
    /// nobody else's: the agent's process and slot are dropped (the next job starts a new one)
    /// and `lost` is the answer. The other helpers' agents never see it.
    async fn isolated<T>(&self, helper: &HelperId, lost: T, work: impl Future<Output = T>) -> T {
        match caught(work, lost).await {
            Caught::Done(done) => done,
            Caught::Lost(lost) => {
                tracing::warn!("a helper's agent failed unexpectedly; its process is ended");
                let gone = {
                    let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
                    slots.remove(helper)
                };
                if let Some(slot) = gone {
                    slot.running.store(false, Ordering::SeqCst);
                    let closing = async { slot.agent.lock().await.close().await };
                    // Ending the process may fail the same way; nothing more is done about it.
                    let _ = caught(closing, ()).await;
                }
                lost
            }
        }
    }

    fn known(&self, helper: &HelperId) -> Option<Slot<H>> {
        let slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
        slots.get(helper).cloned()
    }
}

impl<H, F> AgentApps for HelperAgents<H, F>
where
    H: Hosts + 'static,
    F: Fn(&HelperCard) -> H + Send + Sync,
{
    fn run<'a>(&'a self, card: &'a HelperCard, job: &'a Job) -> AgentPending<'a, AgentOutcome> {
        let lost = AgentOutcome::Failed(JobFault::Crashed);
        Box::pin(self.isolated(&card.id, lost, async move {
            let slot = self.slot(card);
            let mut agent = slot.agent.lock().await;
            slot.running.store(true, Ordering::SeqCst);
            let at = (self.now)();
            let ended = {
                let run = agent.run(job.goal.as_str(), at);
                tokio::pin!(run);
                tokio::select! {
                    ended = &mut run => Some(ended),
                    () = slot.cancel.notified() => None,
                }
            };
            slot.running.store(false, Ordering::SeqCst);
            let Some(ended) = ended else {
                // The turn was dropped half way: ask the agent to cancel it, and end the process
                // so the next job starts clean.
                agent.stop().await;
                agent.close().await;
                return AgentOutcome::Stopped;
            };
            match ended {
                AgentEnd::Done(words) => match agent.handle() {
                    Some((task, session)) => AgentOutcome::Done {
                        words,
                        task,
                        session,
                    },
                    None => AgentOutcome::Failed(JobFault::Crashed),
                },
                AgentEnd::NeedsSignIn(ways) => AgentOutcome::NeedsSignIn(ways),
                AgentEnd::Failed(trouble) => AgentOutcome::Failed(fault_of(trouble)),
                AgentEnd::Stopped => AgentOutcome::Stopped,
            }
        }))
    }

    fn stop<'a>(&'a self, helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async move {
            if let Some(slot) = self.known(helper)
                && slot.running.load(Ordering::SeqCst)
            {
                slot.cancel.notify_one();
            }
        })
    }

    fn sign_in<'a>(
        &'a self,
        card: &'a HelperCard,
        way: &'a SignInWay,
    ) -> AgentPending<'a, SignInOutcome> {
        Box::pin(self.isolated(&card.id, SignInOutcome::Failed, async move {
            let slot = self.slot(card);
            let mut agent = slot.agent.lock().await;
            match agent.sign_in(way).await {
                SignInStep::SignedIn => SignInOutcome::SignedIn,
                SignInStep::OpenTerminal(login) => SignInOutcome::OpenTerminal(login),
                SignInStep::Refused => SignInOutcome::Refused,
                SignInStep::Failed(_) => SignInOutcome::Failed,
            }
        }))
    }

    fn check<'a>(&'a self, card: &'a HelperCard) -> AgentPending<'a, CheckOutcome> {
        Box::pin(self.isolated(&card.id, CheckOutcome::Failed, async move {
            let slot = self.slot(card);
            let mut agent = slot.agent.lock().await;
            match agent.check().await {
                Ok(()) => CheckOutcome::SignedIn,
                Err(AgentEnd::NeedsSignIn(ways)) => CheckOutcome::StillNeedsSignIn(ways),
                Err(_) => CheckOutcome::Failed,
            }
        }))
    }

    fn sign_out<'a>(&'a self, card: &'a HelperCard) -> AgentPending<'a, SignOutOutcome> {
        Box::pin(self.isolated(&card.id, SignOutOutcome::Failed, async move {
            let slot = self.slot(card);
            let mut agent = slot.agent.lock().await;
            match agent.sign_out().await {
                SignOutStep::SignedOut => SignOutOutcome::SignedOut,
                SignOutStep::Failed(_) => SignOutOutcome::Failed,
                SignOutStep::UseLogoutCommand => {
                    let (Some(logout), Some(program)) = (&self.logout, card.kind.program()) else {
                        return SignOutOutcome::Failed;
                    };
                    match logout.run(program).await {
                        true => SignOutOutcome::SignedOut,
                        false => SignOutOutcome::Failed,
                    }
                }
            }
        }))
    }

    fn forget<'a>(&'a self, helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async move {
            let gone = {
                let mut slots = self.slots.lock().unwrap_or_else(|e| e.into_inner());
                slots.remove(helper)
            };
            if let Some(slot) = gone {
                slot.agent.lock().await.close().await;
            }
        })
    }
}
