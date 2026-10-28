//! A helper's job as an ordinary task: its own router session (never wider than the helper's reach
//! and its giver's policy), its goal an instruction for its planner, earlier answers handed to it
//! as data, and its loop run to the end. Each job runs on a `Companion` of its own, so a fault in
//! one is a fault in that job and no other; the host spawns the run and watches it.

use crate::best_effort::best_effort;
use crate::fault::ServeFault;
use crate::helpers::view::role_text;
use crate::runtime::Companion;
use crate::seams::Surface;
use crate::task::Failure;
use agent_loop::{FinishedAs, LoopInput, LoopPhase};
use companion_core::{JobEnding, JobInput, JobReport, JobSpec, OpenedJob};
use companion_wire::{AnswerPhase, NeedsYou, RefusalWire};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{CallVerdict, GoalSource, HelperSeat, JobFault, SeatPlace};
use docket_core::{
    CallEnd, CallRefusal, DraftPart, MessageDraft, SessionOpen, StepEnd, TurnSource, TurnVia,
    UserTurn,
};
use docket_permissions_core::DenyCode;
use docket_planner::Planner;
use docket_vocab::{Clock, TurnId};
use porter_core::Count;
use prov::{Address, AgentRef, MessageKind, MessageText, TaskId};

/// The steps a job may take: a routine's run may be given fewer than the helper's own job, never
/// more. A job past them ends as `JobFault::OutOfSteps`, which reads "it used up its steps".
fn steps_for(helper: Count, run: Option<Count>) -> Count {
    run.map_or(helper, |run| run.min(helper))
}

fn fault_of(failure: Option<&Failure>) -> JobFault {
    match failure {
        Some(Failure::Budget | Failure::Refused(CallRefusal::OverBudget(_))) => {
            JobFault::OutOfSteps
        }
        Some(Failure::Refused(_)) => JobFault::Refused,
        Some(Failure::Model(_) | Failure::Reader) | None => JobFault::NoAnswer,
    }
}

fn why_of(refused: Option<&RefusalWire>) -> Option<String> {
    match refused {
        Some(RefusalWire::Failed(text)) if !text.is_empty() => Some(text.clone()),
        Some(RefusalWire::NoModel) => Some("no model is chosen for it".to_owned()),
        _ => None,
    }
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// Opens the session of a helper's job and briefs its planner: the role and the goal in the
    /// planner's role, the policy cut from the person's words (or the giver's) and the helper's
    /// reach, and the answers it is handed waiting in its inbox. A session that cannot be briefed
    /// is closed again.
    pub async fn open_job(&mut self, spec: &JobSpec) -> Result<OpenedJob, JobFault> {
        self.refresh_catalogue().await;
        self.config.budget.calls = steps_for(spec.card.budget.steps, spec.job.steps);
        self.as_helper = Some(super::AsHelper {
            id: spec.card.id.clone(),
            reach: spec.card.reach.clone(),
        });
        let open = SessionOpen {
            space: spec.card.space.clone(),
            agent: AgentRef::Companion,
            parent: spec.job.giver.clone(),
            cwd: None,
            started_from: None,
            external: None,
            helper: Some(HelperSeat {
                helper: spec.card.id.clone(),
                name: spec.card.name.clone(),
                reach: spec.card.reach.clone(),
                owns: spec.job.owns.iter().cloned().collect(),
                place: SeatPlace::Job,
            }),
            acts_for: None,
        };
        let router = self
            .intents
            .session_open(open.clone())
            .await
            .map_err(|cause| {
                tracing::warn!(%cause, "a helper's job could not open its session");
                JobFault::Crashed
            })?;
        let mine = SessionOpen {
            agent: AgentRef::Worker {
                task: router.task.clone(),
            },
            ..open
        };
        self.adopt(&router, &mine);
        self.publish();
        let opened = OpenedJob {
            task: router.task,
            session: router.session,
        };
        if let Err(fault) = self.brief_job(spec, &opened).await {
            self.dismiss(&opened.task).await;
            return Err(fault);
        }
        Ok(opened)
    }

    async fn brief_job(&mut self, spec: &JobSpec, opened: &OpenedJob) -> Result<(), JobFault> {
        let role = role_text(&spec.card, &spec.job).map_err(|_| JobFault::Crashed)?;
        self.planner.set_role(role);
        // The person's own words give the policy; a planner's words never do, so a job its giver
        // wrote has only its giver's policy, cut to the helper's reach.
        if spec.job.source == GoalSource::Person {
            let turn = UserTurn {
                id: TurnId(0),
                text: spec.job.goal.as_str().to_owned(),
                at: self.clock.now(),
                from: TurnSource::Launcher,
                via: TurnVia::Typed,
                mode: None,
            };
            let narrowed = self
                .intents
                .session_narrow(opened.session.clone(), turn)
                .await;
            best_effort("session.narrow", &opened.session, narrowed);
        }
        // The event that started a routine's run goes to the planner like any other thing it may
        // name and not read: sent from the job's own session, so the router labels it as the
        // planner's and delivers it as a handle.
        let event = spec
            .job
            .about
            .iter()
            .filter_map(|about| serde_json::to_string(about).ok())
            .map(|text| JobInput {
                from: opened.session.clone(),
                text,
            });
        for input in spec.inputs.iter().cloned().chain(event) {
            let draft = MessageDraft {
                to: Address::new(
                    AgentRef::Worker {
                        task: opened.task.clone(),
                    },
                    spec.card.space.clone(),
                ),
                thread: None,
                in_reply_to: None,
                kind: MessageKind::Note,
                parts: vec![DraftPart::Text(MessageText::new(input.text.clone()))],
            };
            let sent = self.intents.send(input.from.clone(), draft).await;
            best_effort("message.send.input", &input.from, sent);
        }
        self.pull().await.map_err(|_| JobFault::Crashed)?;
        Ok(())
    }

    /// Runs the job's loop to the end of what it can do alone and reports how it ended.
    pub async fn run_job(&mut self, opened: &OpenedJob) -> JobReport {
        let ran = self.run(&opened.task, LoopInput::Messaged).await;
        self.job_report(&opened.task, ran)
    }

    pub(super) fn job_report(&self, task: &TaskId, ran: Result<(), ServeFault>) -> JobReport {
        let (Some(state), Some(rt)) = (self.tasks.get(task), self.runtimes.get(task)) else {
            return JobReport::failed(JobFault::Crashed);
        };
        let calls = rt
            .history
            .iter()
            .filter_map(|line| match &line.end {
                StepEnd::Done { .. } => Some(CallVerdict::Allowed),
                StepEnd::Refused(CallRefusal::Denied(_)) => Some(CallVerdict::Denied),
                _ => None,
            })
            .collect();
        let owned = rt
            .steps
            .iter()
            .filter(|step| {
                matches!(
                    &step.end,
                    CallEnd::Refused(CallRefusal::Denied(DenyCode::OwnedByAnotherJob))
                )
            })
            .flat_map(|step| step.targets.iter().cloned())
            .collect();
        let words = || match rt.said.is_empty() {
            true => rt.progress.last().cloned().unwrap_or_default(),
            false => rt.said.join("\n"),
        };
        let (ending, why) = match (ran, state.phase) {
            (Err(_), _) => (JobEnding::Failed(JobFault::Crashed), None),
            (Ok(()), LoopPhase::Finished(FinishedAs::Done)) => {
                (JobEnding::Done { words: words() }, None)
            }
            (Ok(()), LoopPhase::Finished(FinishedAs::Cancelled)) => (JobEnding::Stopped, None),
            (Ok(()), LoopPhase::Finished(FinishedAs::Failed)) => (
                JobEnding::Failed(fault_of(rt.failure.as_ref())),
                why_of(rt.refused.as_ref()),
            ),
            // The planner asked the person something: that is its last word on this job, and the
            // person's answer is a job of its own.
            (Ok(()), LoopPhase::Idle) => match &rt.phase {
                AnswerPhase::NeedsYou(NeedsYou::Question { text, .. }) => (
                    JobEnding::Done {
                        words: text.clone(),
                    },
                    None,
                ),
                _ => (
                    JobEnding::Failed(JobFault::NoAnswer),
                    why_of(rt.refused.as_ref()),
                ),
            },
            (Ok(()), LoopPhase::Paused(_)) => (JobEnding::Failed(JobFault::Refused), None),
            (Ok(()), _) => (JobEnding::Failed(JobFault::Crashed), None),
        };
        if let JobEnding::Failed(fault) = &ending {
            tracing::info!(
                ?fault,
                loop_phase = ?state.phase,
                answer = ?rt.phase,
                failure = ?rt.failure,
                refused = ?rt.refused,
                steps = ?rt
                    .history
                    .iter()
                    .map(|line| (line.action.name.as_str(), &line.end))
                    .collect::<Vec<_>>(),
                "a helper's job did not finish"
            );
        }
        JobReport {
            ending,
            calls,
            owned,
            why,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_runs_budget_only_ever_lowers_the_helpers() {
        assert_eq!(steps_for(Count(12), None), Count(12));
        assert_eq!(steps_for(Count(12), Some(Count(4))), Count(4));
        assert_eq!(steps_for(Count(3), Some(Count(9))), Count(3));
    }
}
