//! The agent: what the builder makes, and the one thing it can do, `ask`.

use crate::builder::AgentBuilder;
use crate::driver::Driver;
use crate::fault::KitFault;
use crate::limits::Limits;
use crate::memory::Memory;
use crate::run::Run;
use agent_loop::LoopInput;
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{
    AssemblerBudget, ContextKeep, Keep, Origin, SessionOpen, TurnIn, TurnSource, TurnVia, UserTurn,
};
use docket_planner::{Catalogue, PlannerModel};
use docket_tasks::TaskRuntime;
use porter_client::Transport as InferTransport;
use porter_core::{AppName, UnixSeconds};
use prov::{AgentRef, SessionId, SpaceId};

/// An agent has no window and shows the person nothing, so no chip of context is kept.
const fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

/// Who asks, where and when. The words are recorded through the router as a turn of this source,
/// so the task policy is derived from them under the caller's role, as for any other prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asker {
    /// The app the agent runs in (the context section names it).
    pub app: AppName,
    /// The Space the task works in.
    pub space: SpaceId,
    /// Where the words come from.
    pub from: TurnSource,
    /// When (passed in: the kit reads no clock).
    pub at: UnixSeconds,
}

/// An agent declared with [`Agent::builder`]. It holds no tool and has no way to run an action:
/// its only effect is a `CallRequest` sent through the router, which gates it.
#[derive(Debug)]
pub struct Agent<P: InferTransport, I: IntentsTransport> {
    pub(crate) intents: Intents<I>,
    pub(crate) planner: PlannerModel<P>,
    pub(crate) memory: Memory,
    pub(crate) budget: AssemblerBudget,
    pub(crate) limits: Limits,
}

impl<P: InferTransport, I: IntentsTransport> Agent<P, I> {
    /// Starts declaring an agent that asks its model through `infer` and reaches the router
    /// through `intents`.
    pub fn builder(infer: P, intents: Intents<I>) -> AgentBuilder<P, I> {
        AgentBuilder::new(infer, intents)
    }

    /// The actions this agent is offered.
    pub fn catalogue(&self) -> &Catalogue {
        self.planner.catalogue()
    }

    /// Runs one ask to the end: a session is opened, `words` recorded as the turn, and the
    /// planner loop (`agent_step`) driven until it is done, asks, is paused or fails. Every call
    /// goes to the router; the session is closed at the end.
    ///
    /// Roles the link needs: `companion` (the session and the planner's calls) and one of the
    /// person's own roles (`field`, `editor`, `launcher`, `cli`), because only those may record a
    /// turn. A link that must stay `companion` alone (an in-app host's) uses
    /// [`Agent::ask_recorded`] instead.
    pub async fn ask(&self, asker: &Asker, words: &str) -> Result<Run, KitFault> {
        let opened = self
            .intents
            .session_open(SessionOpen {
                space: asker.space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            })
            .await
            .map_err(KitFault::Router)?;
        let id = self
            .intents
            .session_turn(
                opened.session.clone(),
                TurnIn {
                    text: words.to_owned(),
                    origin: Origin::AppInternal,
                    keep: keep_nothing(),
                    via: TurnVia::Typed,
                },
            )
            .await
            .map_err(KitFault::Router)?;
        let turn = UserTurn {
            id,
            text: words.to_owned(),
            at: asker.at,
            from: asker.from.clone(),
            via: TurnVia::Typed,
        };
        let run = self.run_turn(asker, opened.session.clone(), turn).await;
        let close = self.intents.session_close(opened.session).await.err();
        Ok(Run { close, ..run })
    }

    /// Runs one ask whose turn the host has already recorded in `session` with a person-role
    /// link of its own. The kit records nothing and opens and closes nothing: the session and the
    /// turn are the host's, and the host closes the session when it is done with it.
    ///
    /// Roles the link needs: `companion` alone. The person's words come from the host's record
    /// (`turn.from` says who spoke), never from this link, which is why it can stay unable to
    /// record a turn. Every call still goes through the router and is gated as in `ask`.
    pub async fn ask_recorded(&self, asker: &Asker, session: SessionId, turn: UserTurn) -> Run {
        self.run_turn(asker, session, turn).await
    }

    async fn run_turn(&self, asker: &Asker, session: SessionId, turn: UserTurn) -> Run {
        let id = turn.id;
        let mut rt = TaskRuntime::new(
            session,
            asker.space.clone(),
            AgentRef::Companion,
            None,
            asker.at,
        );
        rt.turns.push(turn);
        Driver::new(self, asker, rt).run(LoopInput::Asked(id)).await
    }
}
