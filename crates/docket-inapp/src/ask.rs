//! A turn: the person says something, a task runs as far as it can alone, and the host reads the
//! end of it off the task. The loop, the planner, the router calls, the reader and the records are
//! `docket-tasks`'; this is the person's side (the `field` caller records the turn) and the
//! reading of the result.

use crate::agent::{AgentFault, Ending, InAppAgent, Reply};
use crate::sheet::ConfirmSheet;
use action_review::Reviewer;
use agent_loop::{FinishedAs, LoopPhase, LoopState};
use companion_wire::{AnswerPhase, AskWire, NeedsYou};
use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    ContextKeep, Keep, Origin, PolicyWriter, Reader, SessionOpen, TurnIn, TurnSource, TurnVia,
    UserTurn, WindowKey,
};
use docket_planner::PlanFault;
use docket_router::{Clock, GrantStore, MemoryLink};
use docket_tasks::{Failure, TaskRuntime};
use porter_client::Transport as ModelTransport;
use prov::{AgentRef, TaskId};

const fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

/// The window an in-app turn is asked from: the app's own, the one place the sheet can anchor.
fn own_window() -> Option<WindowKey> {
    WindowKey::parse("in-app").ok()
}

/// How a turn ended, read off the task: its loop's phase and what its answer says.
fn ending_of(state: &LoopState, rt: &TaskRuntime) -> Ending {
    let (text, choices) = match &rt.phase {
        AnswerPhase::NeedsYou(NeedsYou::Question { text, choices }) => {
            (text.clone(), choices.clone())
        }
        _ => (String::new(), Vec::new()),
    };
    match state.phase {
        LoopPhase::Finished(FinishedAs::Done) => Ending::Done,
        LoopPhase::Finished(FinishedAs::Failed) => Ending::Failed(
            rt.failure
                .clone()
                .unwrap_or(Failure::Model(PlanFault::Unavailable)),
        ),
        LoopPhase::Finished(FinishedAs::Cancelled) => Ending::Cancelled,
        LoopPhase::Paused(_) => Ending::Paused(text),
        LoopPhase::Idle
        | LoopPhase::Planning
        | LoopPhase::AwaitingCalls
        | LoopPhase::AwaitingReader => Ending::Asked { text, choices },
    }
}

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    /// Opens a task of the companion in the app's Space and makes it the front task. Nothing runs
    /// until the person says something in it ([`InAppAgent::ask_in`]).
    pub async fn new_task(&mut self) -> Result<TaskId, AgentFault> {
        let opened = self
            .tasks
            .open(SessionOpen {
                space: self.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
            })
            .await?;
        Ok(opened.task)
    }

    /// The person says `text` in the front task (a new one when there is none, or it is over); the
    /// agent plans, calls the app's actions through the gate (the sheet asks where the gate says
    /// so) and runs until it is done, asks, or cannot go on.
    pub async fn ask(&mut self, text: &str) -> Result<Reply, AgentFault> {
        let task = match self.front_live() {
            Some(task) => task,
            None => self.new_task().await?,
        };
        self.ask_in(&task, text).await
    }

    /// The person says `text` in a task of their choosing: that task becomes the front task.
    pub async fn ask_in(&mut self, task: &TaskId, text: &str) -> Result<Reply, AgentFault> {
        let rt = self
            .tasks
            .runtimes
            .get(task)
            .ok_or(AgentFault::NoSuchTask)?;
        let (session, said, steps) = (rt.session.clone(), rt.said.len(), rt.history.len());
        let id = self
            .person
            .session_turn(
                session.clone(),
                TurnIn {
                    text: text.to_owned(),
                    origin: Origin::InWindowField,
                    keep: keep_nothing(),
                    via: TurnVia::Typed,
                },
            )
            .await?;
        let turn = UserTurn {
            id,
            text: text.to_owned(),
            at: self.clock.now(),
            from: TurnSource::Field(self.app.clone()),
            via: TurnVia::Typed,
        };
        let parent_window = own_window().ok_or(AgentFault::Manifest)?;
        let asked = self
            .tasks
            .ask(AskWire {
                session: session.clone(),
                turn,
                keep: keep_nothing(),
                parent_window,
                app: Some(self.app.clone()),
            })
            .await;
        let reply = self.reply_of(task, said, steps);
        // A task that is over is dismissed: its router session closes (the episode was left).
        if matches!(
            reply.as_ref().map(|r| &r.ending),
            Some(Ending::Done | Ending::Failed(_) | Ending::Cancelled)
        ) {
            let _ = self.tasks.close(session).await;
        }
        if self.audit == crate::kit::AuditTo::Memory {
            self.flush_audit().await;
        }
        asked?;
        reply.ok_or(AgentFault::NoSuchTask)
    }

    /// What the turn that began with `said` words and `steps` lines left in `task`.
    fn reply_of(&self, task: &TaskId, said: usize, steps: usize) -> Option<Reply> {
        let rt = self.tasks.runtimes.get(task)?;
        let state = self.tasks.tasks.get(task)?;
        Some(Reply {
            task: task.clone(),
            said: rt.said.get(said..).unwrap_or_default().to_vec(),
            steps: rt.history.get(steps..).unwrap_or_default().to_vec(),
            ending: ending_of(state, rt),
        })
    }
}
