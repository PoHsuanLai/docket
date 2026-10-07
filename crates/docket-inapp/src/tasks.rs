//! Many tasks in one app: the front pointer, the roster, the person's words to a subagent, time
//! passing (side conversations that end, the narrative of a finished task), and restart. All of
//! it is `docket-tasks`' (companiond's own model); these are the host's doors to it.

use crate::agent::{AgentFault, InAppAgent};
use crate::sheet::ConfirmSheet;
use action_review::Reviewer;
use agent_loop::LoopPhase;
use companion_wire::FrontTask;
use docket_client::{ContextSource, IntentProvider};
use docket_core::{PolicyWriter, Reader, Roster, TurnSource, TurnVia, UserTurn};
use docket_router::{Clock, GrantStore, MemoryLink};
use porter_client::Transport as ModelTransport;
use prov::{AgentRef, TaskId};

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
    /// The task the next `ask` goes to, and its session.
    pub fn front(&self) -> FrontTask {
        self.tasks.front_task()
    }

    /// The front task when it is still going: a finished one is not asked again.
    pub(crate) fn front_live(&self) -> Option<TaskId> {
        let front = self.tasks.front.clone()?;
        let state = self.tasks.tasks.get(&front)?;
        (!matches!(state.phase, LoopPhase::Finished(_))).then_some(front)
    }

    /// Everyone working or lately done, as the person would see them listed.
    pub fn roster(&self) -> Roster {
        self.tasks.roster()
    }

    /// Where a task's loop stands, if the agent has the task.
    pub fn phase_of(&self, task: &TaskId) -> Option<LoopPhase> {
        self.tasks.tasks.get(task).map(|s| s.phase)
    }

    /// The tasks the agent holds, oldest first by id.
    pub fn open_tasks(&self) -> Vec<TaskId> {
        self.tasks.runtimes.keys().cloned().collect()
    }

    /// Closes a task: one still going is cancelled first, and its router session ends.
    pub async fn close_task(&mut self, task: &TaskId) -> Result<(), AgentFault> {
        let session = self
            .tasks
            .runtimes
            .get(task)
            .map(|rt| rt.session.clone())
            .ok_or(AgentFault::NoSuchTask)?;
        Ok(self.tasks.close(session).await?)
    }

    /// The person said `text` to a subagent of theirs (a side conversation): the roster line shows
    /// it at once, and the conversation is tracked until it goes quiet ([`InAppAgent::tick`]) or
    /// its row is closed ([`InAppAgent::row_closed`]).
    pub fn told(&mut self, to: AgentRef, text: &str) {
        let turn = UserTurn {
            id: self.next_side_turn(),
            text: text.to_owned(),
            at: self.clock.now(),
            from: TurnSource::Field(self.app.clone()),
            via: TurnVia::Typed,
        };
        self.tasks.told(to, self.space.clone(), turn);
    }

    /// The person closed a subagent's row: the side conversation ends now.
    pub async fn row_closed(&mut self, agent: AgentRef) {
        self.tasks.row_closed(agent).await;
    }

    /// Time passed, as the agent's clock says: side conversations that went quiet become
    /// episodes, and a finished task's narrative is written while the person is away.
    pub async fn tick(&mut self) -> Result<(), AgentFault> {
        let ticked = self.tasks.tick().await;
        if self.audit == crate::kit::AuditTo::Memory {
            self.flush_audit().await;
        }
        Ok(ticked?)
    }

    /// A restart: reads what memory holds of the app's Space, puts the agents that were working
    /// back on the roster and opens the front task again as a fresh session. Returns the task it
    /// opened, if there was one to pick up. Memory that does not answer leaves the agent as new.
    pub async fn restore(&mut self) -> Result<Option<TaskId>, AgentFault> {
        let space = self.space.clone();
        let opened = self.tasks.restore(&[space]).await?;
        Ok(opened.map(|o| o.task))
    }

    /// How many side conversations are open.
    pub fn side_conversations(&self) -> usize {
        self.tasks.side.open.len()
    }
}
