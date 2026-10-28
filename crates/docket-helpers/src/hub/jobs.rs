//! The hub runs the jobs the board launches. A job runs as a task of its own (started through the
//! `Spawn` seam) on a `Companion` of its own (built by the factory the daemon passes in), so several helpers work at
//! once, none behind the front conversation's lock, and a panic or an error in one job ends that
//! job: a second task awaits the first and turns what it finds, a result or a `Fate`, into
//! the board's event for that job (`companion_core::conclude`). The daemon and every other job go
//! on.

use super::owner::{Hub, Running};
use crate::spawn::{Fate, Watch, Work};
use companion_core::{
    Conclusion, Finished, Hold, JobEnding, JobReport, JobSpec, OpenedJob, conclude, job_spec,
    same_app,
};
use companion_model::helper::{
    AgentApps, AgentOutcome, BoardEvent, HelperName, JobFault, JobId, Launch, TaskRef,
    owned_sentence,
};
use docket_client::Transport as IntentsTransport;
use docket_planner::Planner;
use docket_tasks::Surface;
use docket_vocab::Clock;
use prov::{AgentProgram, HelperId, SessionId, TaskId};
use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Duration;

/// How long a stopped job has to end by itself before its task is cut off.
const GRACE: Duration = Duration::from_secs(5);

impl<P, I, K, S> Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// Starts the task that runs a job, and the task that watches it.
    pub(super) fn launch(&self, launch: &Launch) {
        let Some(spec) = self.with_state(|state| job_spec(&state.board, &state.answers, launch))
        else {
            return;
        };
        let job = launch.job;
        self.with_state(|state| {
            state.running.insert(
                job,
                Running {
                    helper: launch.helper.clone(),
                    cutoff: None,
                    task: None,
                    session: None,
                    shared: None,
                },
            );
        });
        let hub = self.clone();
        let work: Work = Box::pin(async move { hub.work(spec).await });
        let hub = self.clone();
        let watch: Watch = Box::new(move |outcome| {
            Box::pin(async move {
                hub.ended(job, outcome).await;
            })
        });
        let cutoff = self.inner.spawn.run_watched(work, watch);
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.cutoff = Some(cutoff);
            }
        });
    }

    /// Gives the hub the host of the agent apps that are helpers. A helper whose kind is an app
    /// has its jobs run there; without one such a job fails at once, and nothing else changes.
    pub fn set_agents(&self, agents: Arc<dyn AgentApps>) {
        // Set once: a second host is ignored rather than swapped under running jobs.
        let _ = self.inner.agents.set(agents);
    }

    pub(super) fn agents(&self) -> Option<Arc<dyn AgentApps>> {
        self.inner.agents.get().cloned()
    }

    /// One job, from its session to its report: the body of the task that runs it.
    async fn work(&self, spec: JobSpec) -> Finished {
        match spec.card.kind.program() {
            Some(_) => self.work_agent(spec).await,
            None => self.work_native(spec).await,
        }
    }

    /// A job of a helper that runs an agent app: the goal is the app's one turn, and what it says
    /// comes back as the job's answer. The app's session at the router is the app process's, so
    /// the hub never closes it. A job that cannot start because the app is not signed in is the
    /// board's `NeedsSignIn`, never a failure.
    async fn work_agent(&self, spec: JobSpec) -> Finished {
        let job = spec.job.id;
        let unstarted = |fault| Finished {
            opened: None,
            report: JobReport::failed(fault),
            hold: Hold::Leave,
        };
        let Some(agents) = self.agents() else {
            return unstarted(JobFault::Crashed);
        };
        // The app has no task of ours; the card still names one, so a pause finds the job.
        let Ok(task) = TaskId::parse(&format!("agent-job-{}", job.0)) else {
            return unstarted(JobFault::Crashed);
        };
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.task = Some(task.clone());
            }
        });
        self.apply(BoardEvent::Started { job, task }).await;
        let (ending, opened) = match agents.run(&spec.card, &spec.job).await {
            AgentOutcome::Done {
                words,
                task,
                session,
                own_session,
            } => {
                self.with_state(|state| match own_session {
                    Some(_) => state.terminals.can_pick_up(&spec.card.id),
                    None => state.terminals.cannot_pick_up(&spec.card.id),
                });
                (
                    JobEnding::Done {
                        words: words.into_inner(),
                    },
                    Some(OpenedJob { task, session }),
                )
            }
            AgentOutcome::NeedsSignIn(ways) => (JobEnding::NeedsSignIn(ways), None),
            AgentOutcome::Failed(fault) => (JobEnding::Failed(fault), None),
            AgentOutcome::Stopped => (JobEnding::Stopped, None),
        };
        Finished {
            opened,
            report: JobReport {
                ending,
                calls: Vec::new(),
                owned: Vec::new(),
                why: None,
            },
            hold: Hold::Leave,
        }
    }

    async fn work_native(&self, spec: JobSpec) -> Finished {
        let job = spec.job.id;
        let mut companion = self.helpers_companion(&spec.card);
        let opened = match companion.open_job(&spec).await {
            Ok(opened) => opened,
            Err(fault) => {
                return Finished {
                    opened: None,
                    report: JobReport::failed(fault),
                    hold: Hold::Close,
                };
            }
        };
        let shared = companion.shared.clone();
        self.with_state(|state| {
            if let Some(running) = state.running.get_mut(&job) {
                running.task = Some(opened.task.clone());
                running.session = Some(opened.session.clone());
                running.shared = Some(shared);
            }
        });
        self.apply(BoardEvent::Started {
            job,
            task: opened.task.clone(),
        })
        .await;
        let report = companion.run_job(&opened).await;
        Finished {
            opened: Some(opened),
            report,
            hold: Hold::Close,
        }
    }

    /// A job's task ended, by itself or not: tell the board how, and close what is not kept. A
    /// task that panicked ends its job as failed, with a plain sentence on its helper; a task that
    /// was cut off ends it as stopped. Nothing else is touched.
    async fn ended(&self, job: JobId, outcome: Result<Finished, Fate>) {
        let Some(running) = self.with_state(|state| state.running.remove(&job)) else {
            return;
        };
        let conclusion = match outcome {
            Ok(done) => conclude(job, done),
            Err(Fate::Cut) => Conclusion::cut_off(job, running.session.clone()),
            Err(Fate::Panicked) => {
                tracing::error!(%job, "a helper's job panicked; only that job ends");
                Conclusion::crashed(job, running.session.clone())
            }
        };
        let Conclusion {
            event,
            answer,
            close,
            calls,
            owned,
            why,
            ways,
        } = conclusion;
        // An agent app that answered is signed in: the agent said so by opening its session.
        let answered_by_app = answer.as_ref().is_some_and(|a| a.hold == Hold::Leave);
        let evicted = answer
            .map(|answer| self.with_state(|state| state.answers.keep(job, answer)))
            .unwrap_or_default();
        if let Some(ways) = ways {
            self.with_state(|state| state.logins.wants_sign_in(&running.helper, ways));
        }
        if let Some(why) = &why {
            self.with_state(|state| state.answers.note_why(job, why.clone()));
        }
        self.apply_saying(event, why).await;
        if answered_by_app {
            self.signed_in_to_app_of(&running.helper).await;
        }
        for verdict in calls {
            self.apply(BoardEvent::Called {
                helper: running.helper.clone(),
                verdict,
            })
            .await;
        }
        // Each owner is named once, however many of its things were reached for.
        let owners: BTreeSet<HelperName> = self.with_state(|state| {
            owned
                .iter()
                .filter_map(|thing| state.board.owner_of(thing, Some(job)))
                .map(|(card, _)| card.name.clone())
                .collect()
        });
        for owner in owners {
            self.tell(&running.helper, owned_sentence(&owner));
        }
        for session in close.into_iter().chain(evicted) {
            self.close_session(session).await;
        }
    }

    /// The words a finished job answered, while they are kept.
    pub fn answer_words(&self, job: JobId) -> Option<String> {
        self.with_state(|state| state.answers.words(job).map(str::to_owned))
    }

    /// What is kept of a job's words: its answer, and why it stopped.
    pub fn job_words(&self, job: JobId) -> companion_wire::JobWords {
        self.with_state(|state| companion_wire::JobWords {
            answer: state.answers.words(job).map(str::to_owned),
            why: state.answers.why(job).map(str::to_owned),
        })
    }

    /// The agent app `helper` runs is signed in. It has one login, so every helper that runs the
    /// same app is: those that waited for it take their jobs now.
    pub(super) async fn signed_in_to_app_of(&self, helper: &HelperId) {
        let Some(program) = self.program_of(helper) else {
            return;
        };
        let same = self.with_state(|state| {
            let same = same_app(&state.board, &program);
            state.logins.signed_in(same.all.iter().cloned());
            same
        });
        for id in same.waiting {
            self.apply(BoardEvent::SignedIn(id)).await;
        }
    }

    /// The agent app `helper` runs, if it runs one.
    pub(super) fn program_of(&self, helper: &HelperId) -> Option<AgentProgram> {
        self.with_state(|state| state.board.card(helper)?.kind.program().cloned())
    }

    async fn close_session(&self, session: SessionId) {
        let service = self.inner.service.lock().await;
        let closed = service.intents.session_close(session.clone()).await;
        if let Err(cause) = closed {
            tracing::warn!(%session, %cause, "a helper's session would not close");
        }
    }

    /// Stops a running job: its task is asked to end, and cut off if it has not after a short
    /// while (a call the router already carries out is not abandoned half way).
    pub(super) fn stop(&self, at: &TaskRef) {
        let found = self.with_state(|state| {
            state.running.get(&at.job).map(|r| {
                (
                    r.shared.clone(),
                    r.task.clone(),
                    r.cutoff.clone(),
                    r.helper.clone(),
                )
            })
        });
        let Some((shared, task, cutoff, helper)) = found else {
            return;
        };
        if let (Some(shared), Some(task)) = (shared, task) {
            shared.cancel(&task);
        }
        // A job on an agent app is stopped at the app; for any other helper this finds nothing.
        if let Some(agents) = self.agents() {
            self.inner
                .spawn
                .detach(Box::pin(async move { agents.stop(&helper).await }));
        }
        if let Some(cutoff) = cutoff {
            self.inner
                .spawn
                .detach_after(GRACE, Box::pin(async move { cutoff.cut() }));
        }
    }
}
