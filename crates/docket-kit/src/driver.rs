//! Carrying out the loop for one task. `agent_step` says what happens; this turns each effect
//! into a call to the router or the planner and feeds the end back in, until the task is over or
//! waiting. Nothing here decides what is allowed: every call goes to the router, and a refusal
//! comes back as a code. It is the small sibling of `docket-tasks`' drive loop, built on the same
//! pieces (`TaskRuntime`, `answer_read`, `refusal_of`).

use crate::agent::{Agent, Asker};
use crate::memory::Where;
use crate::run::{Ended, Run};
use agent_loop::{
    FinishedAs, LoopEffect, LoopInput, LoopPhase, LoopState, ModelOutput, Sources, agent_step,
    assemble,
};
use companion_wire::{AnswerPhase, NeedsYou};
use docket_client::Transport as IntentsTransport;
use docket_core::{CallId, CallRefusal, CallRequest, NoteAsk, Roster};
use docket_tasks::{Failure, TaskRuntime, answer_read, idle_state, nowhere, refusal_of};
use porter_client::Transport as InferTransport;
use prov::Effect;
use std::collections::VecDeque;

pub(crate) struct Driver<'a, P: InferTransport, I: IntentsTransport> {
    agent: &'a Agent<P, I>,
    asker: &'a Asker,
    rt: TaskRuntime,
    state: LoopState,
}

impl<'a, P: InferTransport, I: IntentsTransport> Driver<'a, P, I> {
    pub(crate) fn new(agent: &'a Agent<P, I>, asker: &'a Asker, rt: TaskRuntime) -> Self {
        Self {
            agent,
            asker,
            rt,
            state: idle_state(),
        }
    }

    /// Runs `first`, and what follows from it, to the end of what the task can do alone.
    pub(crate) async fn run(mut self, first: LoopInput) -> Run {
        let mut queue = VecDeque::from([first]);
        while let Some(input) = queue.pop_front() {
            let (next, effects) = agent_step(self.state.clone(), input);
            self.state = next;
            let mut position = 0u64;
            for effect in effects {
                queue.extend(self.carry_out(effect, &mut position).await);
            }
        }
        self.into_run()
    }

    fn into_run(self) -> Run {
        let question = match &self.rt.phase {
            AnswerPhase::NeedsYou(NeedsYou::Question { text, choices }) => {
                Some((text.clone(), choices.clone()))
            }
            _ => None,
        };
        let ended = match (self.state.phase, question) {
            (LoopPhase::Finished(FinishedAs::Done), _) => Ended::Done,
            (LoopPhase::Finished(FinishedAs::Cancelled), _) => Ended::Cancelled,
            (LoopPhase::Paused(_), question) => Ended::Paused {
                text: question.map(|q| q.0).unwrap_or_default(),
            },
            (LoopPhase::Idle, Some((text, choices))) => Ended::Asked { text, choices },
            _ => Ended::Failed,
        };
        Run {
            ended,
            said: self.rt.said,
            steps: self.rt.history,
            failure: self.rt.failure,
        }
    }

    async fn carry_out(&mut self, effect: LoopEffect, position: &mut u64) -> Vec<LoopInput> {
        match effect {
            LoopEffect::AskPlanner => self.plan_turn().await,
            LoopEffect::Call(call) => {
                let id = CallId(*position);
                *position += 1;
                vec![self.perform(*call, id).await]
            }
            LoopEffect::Read(ask) => answer_read(&self.agent.intents, &mut self.rt, *ask).await,
            LoopEffect::Publish(phase) => {
                self.rt.phase = phase;
                Vec::new()
            }
            LoopEffect::Held(call, why) => {
                let effect = self.effect_of(&call);
                self.rt.hold(&call, effect, why);
                Vec::new()
            }
            LoopEffect::Unread(fault) => {
                self.rt.unread(fault);
                Vec::new()
            }
            LoopEffect::Refused(refusal) => {
                if matches!(refusal, CallRefusal::OverBudget(_)) {
                    self.rt.failure = Some(Failure::Refused(refusal));
                }
                Vec::new()
            }
            LoopEffect::Note(_) => Vec::new(),
            LoopEffect::CloseTask => {
                let _ = self
                    .agent
                    .intents
                    .session_note(self.rt.session.clone(), NoteAsk::End)
                    .await;
                Vec::new()
            }
        }
    }

    fn effect_of(&self, call: &CallRequest) -> Effect {
        self.agent
            .planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect)
    }

    /// One planner turn: read the sections, assemble the view, ask the model, and hand the loop
    /// what it said.
    async fn plan_turn(&mut self) -> Vec<LoopInput> {
        if self.state.steps >= self.agent.limits.steps.0 {
            self.rt.failure = Some(Failure::Budget);
            return vec![LoopInput::ModelFailed];
        }
        let sources = self.sources().await;
        let view = assemble(&self.agent.budget, &sources);
        let reply = match self.agent.planner.converse(&view).await {
            Ok(reply) => reply,
            Err(fault) => {
                self.rt.failure = Some(Failure::Model(fault));
                return vec![LoopInput::ModelFailed];
            }
        };
        self.rt.served = reply.served.clone();
        if let Some(words) = &reply.said {
            self.rt.said.push(words.clone());
        }
        let said = reply.said.map(|w| LoopInput::Planned(ModelOutput::Say(w)));
        said.into_iter()
            .chain([LoopInput::Planned(reply.then)])
            .collect()
    }

    /// The sections for one turn. A source that does not answer leaves its section empty.
    async fn sources(&mut self) -> Sources {
        let agent = self.agent;
        let session = self.rt.session.clone();
        if let Ok(cards) = agent.intents.session_handles(session.clone()).await {
            self.rt.handles = cards;
        }
        let task_policy = agent
            .intents
            .session_task_policy(session.clone())
            .await
            .unwrap_or_default();
        let words = self
            .rt
            .turns
            .iter()
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let at = Where {
            session: &session,
            space: &self.rt.space,
            now: self.asker.at,
            words: &words,
        };
        let filled = agent.memory.fill(&agent.intents, &agent.budget, &at).await;
        Sources {
            cards: agent.planner.catalogue().cards(),
            profile: filled.profile,
            primer: filled.primer,
            rollup: None,
            roster: Roster::default(),
            episodes: filled.episodes,
            recalled: filled.recalled,
            context: nowhere(self.asker.app.clone()),
            turns: self.rt.turns.clone(),
            history: self.rt.history.clone(),
            handles: self.rt.handles.clone(),
            inbox: self.rt.inbox.clone(),
            taint: self.rt.taint(),
            task_policy,
            skills: Vec::new(),
            skill_texts: Vec::new(),
        }
    }

    /// One call, through the router, and how it ended as the loop is told. A call to an action
    /// the agent was not offered is refused here as well, though the planner never reads one.
    async fn perform(&mut self, call: CallRequest, id: CallId) -> LoopInput {
        let offered = self.agent.planner.catalogue().of(&call.action).is_some();
        let effect = self.effect_of(&call);
        let result = if offered {
            match self
                .agent
                .intents
                .perform(call.clone(), Some(self.rt.session.clone()), None)
                .await
            {
                Ok(end) => end,
                Err(error) => Err(refusal_of(error)),
            }
        } else {
            Err(CallRefusal::NoSuchAction(call.action.clone()))
        };
        LoopInput::CallEnded(id, self.rt.record_call(&call, effect, result))
    }
}
