//! A session the log holds, taken up again by a companion that has no task for it: after a
//! restart, or in a process that did not open it. The router brings its own record back when a
//! request names the session (`Router::restore_named`); this gives the task a runtime that
//! matches the plan, so the next turn is planned from the person's words and the steps the log
//! kept, with every handle a label and every call the crash cut off ended interrupted.

use crate::fault::ServeFault;
use crate::linger::dismissed;
use crate::runtime::{Companion, idle_state};
use crate::seams::{Now, Surface};
use crate::task::TaskRuntime;
use agent_loop::{LoopPhase, LoopState};
use companion_wire::AnswerPhase;
use docket_client::Transport as IntentsTransport;
use docket_core::{StepEnd, StepLine, StepShown};
use docket_session::{ResumePlan, Standing};
use porter_client::Transport as InferTransport;
use prov::{AgentRef, SessionId, TaskId};

/// The history of a plan: the calls that ended, and those the crash cut off, in call order.
fn history_of(plan: &ResumePlan) -> Vec<StepLine> {
    let cut = plan.interrupted.iter().map(|i| StepLine {
        call: i.call,
        action: i.open.action.clone(),
        effect: i.open.effect,
        end: StepEnd::Interrupted,
        shown: StepShown::Full,
        with: Vec::new(),
    });
    let mut lines: Vec<StepLine> = plan.history.iter().cloned().chain(cut).collect();
    lines.sort_by_key(|l| l.call);
    lines
}

/// The loop a stored session starts from: waiting, or paused where the breaker left it.
fn loop_of(standing: &Standing) -> LoopState {
    match standing {
        Standing::Paused(trip) => LoopState {
            phase: LoopPhase::Paused(*trip),
            ..idle_state()
        },
        Standing::Open | Standing::Closed(_) | Standing::Blocked(_) => idle_state(),
    }
}

impl<P: InferTransport, I: IntentsTransport, K: Now, S: Surface> Companion<P, I, K, S> {
    /// A finished task whose router session is still open takes another turn: its loop starts
    /// again and it no longer waits to be closed.
    pub(crate) fn reopen(&mut self, task: &TaskId) {
        let finished = matches!(
            self.tasks.get(task).map(|s| s.phase),
            Some(LoopPhase::Finished(_))
        );
        if finished {
            self.tasks.insert(task.clone(), idle_state());
            self.lingering = dismissed(std::mem::take(&mut self.lingering), task);
        }
    }

    /// Takes up the task of a stored session. The router is asked for the session's handles,
    /// which restores its record from the log when it holds none; a router that has no such
    /// session after that cannot run it.
    pub(crate) async fn adopt_stored(
        &mut self,
        session: &SessionId,
        plan: &ResumePlan,
    ) -> Result<TaskId, ServeFault> {
        self.refresh_catalogue().await;
        self.intents
            .session_handles(session.clone())
            .await
            .map_err(ServeFault::Router)?;
        let opening = &plan.opening;
        let mut rt = TaskRuntime::new(
            session.clone(),
            opening.space.clone(),
            opening.agent.clone().unwrap_or(AgentRef::Companion),
            opening.parent.clone(),
            self.clock.now(),
        );
        rt.turns = plan.turns.clone();
        rt.history = history_of(plan);
        rt.next_call = rt.history.iter().map(|l| l.call.0 + 1).max().unwrap_or(0);
        rt.loaded = plan.skills.iter().map(|(id, _)| id.clone()).collect();
        rt.phase = AnswerPhase::Done;
        let task = opening.task.clone();
        self.runtimes.insert(task.clone(), rt);
        self.tasks.insert(task.clone(), loop_of(&plan.standing));
        self.publish_answer(&task);
        Ok(task)
    }

    /// Runs a turn that `begin_ask` began, telling `tap` of each step.
    pub(crate) async fn run_begun_tapped<T: crate::tap::Tap>(
        &mut self,
        begun: crate::runtime::Begun,
        tap: &mut T,
    ) -> Result<(), ServeFault> {
        self.record(&begun.session, &begun.asked).await;
        self.run_interactive_tapped(&begun.task, begun.first, tap)
            .await
    }
}
