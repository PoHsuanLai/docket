//! The turn's loop: the agent-loop machine's effects carried out against the planner, the router
//! (through the two callers) and the quarantined reader.

use crate::agent::{Ending, Failure, InAppAgent};
use crate::sheet::ConfirmSheet;
use crate::turn::OpenTask;
use action_review::Reviewer;
use agent_loop::{FinishedAs, LoopEffect, LoopInput, LoopPhase, ModelOutput, agent_step, assemble};
use companion_wire::{AnswerPhase, NeedsYou};
use docket_client::ClientError;
use docket_client::{ContextSource, IntentProvider};
use docket_core::{
    CallId, CallRefusal, CallRequest, PolicyWriter, ReadAsk, Reader, ReaderAsk, WireRefusal,
};
use docket_planner::PlanFault;
use docket_router::{Clock, GrantStore, MemoryLink};
use porter_client::Transport as ModelTransport;
use prov::Effect;
use std::collections::VecDeque;

fn refusal_of(error: ClientError) -> CallRefusal {
    match error {
        ClientError::Refused(WireRefusal::Call(refusal)) => refusal,
        ClientError::Refused(_) | ClientError::Transport(_) | ClientError::Unexpected => {
            CallRefusal::Timeout
        }
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
    pub(crate) async fn run(&self, mut task: OpenTask, first: LoopInput) -> (Ending, OpenTask) {
        let mut queue = VecDeque::from([first]);
        let mut question = None;
        let mut failure = None;
        while let Some(input) = queue.pop_front() {
            let (next, effects) = agent_step(task.state.clone(), input);
            task.state = next;
            let mut position = 0u64;
            for effect in effects {
                match effect {
                    LoopEffect::AskPlanner => {
                        queue.extend(self.plan_turn(&mut task, &mut failure).await);
                    }
                    LoopEffect::Call(call) => {
                        let id = CallId(position);
                        position += 1;
                        queue.push_back(self.call(&mut task, *call, id).await);
                    }
                    LoopEffect::Read(ask) => {
                        queue.push_back(self.read(&mut task, *ask, &mut failure).await);
                    }
                    LoopEffect::Publish(AnswerPhase::NeedsYou(NeedsYou::Question {
                        text,
                        choices,
                    })) => question = Some((text, choices)),
                    LoopEffect::Refused(refusal) => {
                        if matches!(refusal, CallRefusal::OverBudget(_)) {
                            failure = Some(Failure::Refused(refusal));
                        }
                    }
                    LoopEffect::Held(call, why) => {
                        let effect = self.effect_of(&call);
                        task.hold(&call, effect, why);
                    }
                    LoopEffect::Unread(fault) => task.unread(fault),
                    LoopEffect::Publish(_) | LoopEffect::Note(_) | LoopEffect::CloseTask => {}
                }
            }
        }
        let ending = match task.state.phase {
            LoopPhase::Finished(FinishedAs::Done) => Ending::Done,
            LoopPhase::Finished(FinishedAs::Failed) => {
                Ending::Failed(failure.unwrap_or(Failure::Model(PlanFault::Unavailable)))
            }
            LoopPhase::Finished(FinishedAs::Cancelled) => Ending::Cancelled,
            LoopPhase::Paused(_) => Ending::Paused(question.map(|(t, _)| t).unwrap_or_default()),
            LoopPhase::Idle
            | LoopPhase::Planning
            | LoopPhase::AwaitingCalls
            | LoopPhase::AwaitingReader => {
                let (text, choices) = question.unwrap_or_default();
                Ending::Asked { text, choices }
            }
        };
        (ending, task)
    }

    async fn plan_turn(
        &self,
        task: &mut OpenTask,
        failure: &mut Option<Failure>,
    ) -> Vec<LoopInput> {
        if task.state.steps >= self.config.budget.calls.0 {
            *failure = Some(Failure::Budget);
            return vec![LoopInput::ModelFailed];
        }
        self.refresh_handles(task).await;
        let sources = self.sources(task).await;
        let view = assemble(&self.config.assembler, &sources);
        match self.planner.converse(&view).await {
            Err(fault) => {
                *failure = Some(Failure::Model(fault));
                vec![LoopInput::ModelFailed]
            }
            Ok(reply) => {
                let said = reply.said.map(|words| {
                    task.said.push(words.clone());
                    LoopInput::Planned(ModelOutput::Say(words))
                });
                said.into_iter()
                    .chain([LoopInput::Planned(reply.then)])
                    .collect()
            }
        }
    }

    pub(crate) async fn refresh_handles(&self, task: &mut OpenTask) {
        if let Ok(cards) = self.intents.session_handles(task.session.clone()).await {
            task.handles = cards;
        }
    }

    fn effect_of(&self, call: &CallRequest) -> Effect {
        self.planner
            .catalogue()
            .of(&call.action)
            .map_or(Effect::Read, |t| t.decl.effect)
    }

    /// One call, through the router: gated, reviewed, confirmed on the app's sheet where the
    /// gate says so, performed by the app's own provider.
    async fn call(&self, task: &mut OpenTask, call: CallRequest, id: CallId) -> LoopInput {
        let effect = self.effect_of(&call);
        let result = self
            .intents
            .perform(call.clone(), Some(task.session.clone()), None)
            .await
            .unwrap_or_else(|error| Err(refusal_of(error)));
        LoopInput::CallEnded(id, task.record(call.action, effect, result))
    }

    async fn read(
        &self,
        task: &mut OpenTask,
        ask: ReaderAsk,
        failure: &mut Option<Failure>,
    ) -> LoopInput {
        match self
            .intents
            .session_read(task.session.clone(), ReadAsk { ask })
            .await
        {
            Ok(reveal) => LoopInput::ReadAnswered(reveal),
            Err(_) => {
                *failure = Some(Failure::Reader);
                LoopInput::ModelFailed
            }
        }
    }
}
