//! The runtime: the entry points the bus calls and the effects of the machines.

use crate::planner::PlannerModel;
use agent_loop::{IdleState, LoopState, SideTable};
use companion_wire::{AskWire, FrontTask};
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{AgentConfig, Roster, SessionOpen, SessionOpened};
use porter_client::Transport as InferTransport;
use prov::TaskId;
use std::collections::BTreeMap;

/// The companion.
#[derive(Debug)]
pub struct Companiond<P: InferTransport, I: IntentsTransport> {
    /// The router, as role `companion`.
    pub intents: Intents<I>,
    /// The planner.
    pub planner: PlannerModel<P>,
    /// The proposed values.
    pub config: AgentConfig,
    /// The task the launcher returns to.
    pub front: Option<TaskId>,
    /// Each task's loop.
    pub tasks: BTreeMap<TaskId, LoopState>,
    /// The person's side conversations with subagents.
    pub side: SideTable,
    /// The background narrative pass.
    pub idle: IdleState,
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// `Companion1.Open`: opens a session for the front conversation.
    pub async fn open(&mut self, open: SessionOpen) -> Result<SessionOpened, crate::ServeFault> {
        let _ = open;
        todo!(
            "Companiond::open: Intents1.Session.Open, make the new task the front, record SessionRecord::Opened"
        )
    }

    /// `Companion1.Ask`: starts the loop for a turn the UI already recorded and answers the
    /// object path of its answer.
    pub async fn ask(&mut self, ask: AskWire) -> Result<String, crate::ServeFault> {
        let _ = ask;
        todo!(
            "Companiond::ask: LoopInput::Asked, assemble from Session.Recall (Recent, Inject), Context.Current, the roster and the inbox; carry out the effects"
        )
    }

    /// `Intents1.Message.Arrived`: reads the inbox and feeds each message to the right task as
    /// input (a request is evaluated under that task's own policy; nothing in it widens it).
    pub async fn arrived(&mut self) -> Result<(), crate::ServeFault> {
        todo!(
            "Companiond::arrived: Message.Inbox for each agent here, LoopInput per message, completion notes for reports"
        )
    }

    /// The roster as the active Space may see it (another Space shows presence only).
    pub fn roster(&self) -> Roster {
        todo!("Companiond::roster: Intents1 task table through roster_of, cut by Roster::seen_from")
    }

    /// `Companion1.Front`.
    pub fn front_task(&self) -> FrontTask {
        FrontTask {
            task: self.front.clone(),
            session: None,
        }
    }

    /// Time passed: ends idle side conversations and starts a narrative when the person is away.
    pub async fn tick(&mut self) -> Result<(), crate::ServeFault> {
        todo!(
            "Companiond::tick: side_step(Tick) and idle_step(Tick), carry out WriteEpisode (Session.Note) and Start (reader-shaped narrative as background work)"
        )
    }
}
