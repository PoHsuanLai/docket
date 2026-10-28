//! Handing a turn to an outside coding agent. When the person chose one as the assistant's model
//! (`agent/<program>` in inferd's picker), inferd declines the planner's request naming it
//! ([`DeclinedBecause::AgentChosen`]); the companion then does not plan the turn itself. It gives
//! the person's words to the agent's host through [`Handoff`], shows what the agent said as the
//! answer, and names the agent in the route ("Answered by Claude Code").
//!
//! The host, not the companion, opens the agent's session at the router and makes its calls. The
//! companion hands over words it already holds (the turn the shell recorded) and opens no session
//! of an outside agent.

use crate::runtime::Companion;
use crate::seams::Surface;
use crate::task::Failure;
use agent_loop::{LoopInput, ModelOutput};
use companion_wire::{RefusalWire, RouteNote, WhyWord};
use docket_client::Transport as IntentsTransport;
use docket_core::{HandOver, HandRefusal, HandReply};
use docket_permissions_core::ProgramName;
use docket_planner::{PlanFault, Planner};
use docket_store::Stop;
use docket_vocab::Clock;
use porter_capability::{Locality, ModelId};
use porter_core::AccountId;
use porter_infer::{AGENT_ACCOUNT, DeclinedBecause, ModelLabel, ServedBy, StageRole};
use prov::TaskId;
use std::future::Future;
use std::pin::Pin;

/// What gives a turn to the agent's host.
pub trait Handoff: std::fmt::Debug + Send + Sync {
    /// Gives `ask.turn` to the agent `ask.program`; resolves when the agent has finished the
    /// turn, or the host said why it did not. A host that cannot be reached is a refusal too.
    fn hand_over(&self, ask: HandOver) -> Pin<Box<dyn Future<Output = HandReply> + Send + '_>>;
}

/// The agent the person chose, when `fault` is inferd declining the request because they did.
pub(crate) fn chosen_agent(fault: &PlanFault) -> Option<ProgramName> {
    match fault {
        PlanFault::Declined(declined) if declined.because == DeclinedBecause::AgentChosen => {
            declined
                .model
                .agent_program()
                .and_then(|program| ProgramName::parse(program).ok())
        }
        _ => None,
    }
}

/// The route of a turn the agent answered: "Answered by Claude Code (you chose it)". The agent
/// runs on this computer, in its sandbox; where its own model runs is its business.
pub(crate) fn route_of(program: &ProgramName, label: &str) -> Vec<RouteNote> {
    let (Ok(account), Ok(model)) = (
        AccountId::parse(AGENT_ACCOUNT),
        ModelId::parse(program.as_str()),
    ) else {
        return Vec::new();
    };
    vec![RouteNote {
        stage: StageRole::Answer,
        served: ServedBy {
            account,
            model,
            locality: Locality::OnDevice,
        },
        name: Some(ModelLabel(label.to_owned())),
        why: vec![WhyWord::Named],
        reached: None,
    }]
}

impl<P: Planner, I: IntentsTransport, K: Clock, S: Surface> Companion<P, I, K, S> {
    /// The planner's turn, given to the coding agent the person chose: the agent's words are the
    /// reply, the route names it, and the loop ends as it does for any reply. A host that is not
    /// there, or that refuses, ends the turn the way a model that cannot answer does.
    pub(crate) async fn give_turn(
        &mut self,
        task: &TaskId,
        program: ProgramName,
    ) -> Vec<LoopInput> {
        let Some(hand) = self.hand.clone() else {
            return self.not_handed(task, HandRefusal::NotSetUp);
        };
        let Some((turn, space)) = self
            .runtimes
            .get(task)
            .and_then(|rt| Some((rt.turns.last()?.clone(), rt.space.clone())))
        else {
            return self.not_handed(task, HandRefusal::NotSetUp);
        };
        let ask = HandOver {
            program: program.clone(),
            space,
            turn,
        };
        let asked = crate::deadline::bounded(
            &*self.shared,
            &self.clock,
            task,
            self.waits.person,
            hand.hand_over(ask),
        )
        .await;
        match asked {
            Err(Stop::Cancelled) => vec![LoopInput::Cancelled],
            Err(Stop::TimedOut) => self.not_handed(
                task,
                HandRefusal::Failed {
                    message: "The coding agent took too long to answer.".to_owned(),
                },
            ),
            Ok(HandReply::Handed(handed)) => {
                if let Some(rt) = self.runtimes.get_mut(task) {
                    rt.served = None;
                    rt.route = route_of(&program, &handed.label);
                    rt.said.push(handed.words.clone());
                }
                vec![
                    LoopInput::Planned(ModelOutput::Say(handed.words)),
                    LoopInput::Planned(ModelOutput::Finish),
                ]
            }
            Ok(HandReply::Refused(refusal)) => self.not_handed(task, refusal),
        }
    }

    /// The turn ended because the agent did not take it. A switched-off or missing agent asks the
    /// person to choose a model (the picker shows why that row can't be used); the rest say what
    /// happened.
    fn not_handed(&mut self, task: &TaskId, refusal: HandRefusal) -> Vec<LoopInput> {
        tracing::warn!(%task, why = ?refusal, "the coding agent did not take the turn");
        let (shown, fault) = match &refusal {
            HandRefusal::TurnedOff | HandRefusal::NotSetUp => {
                (RefusalWire::NoModel, PlanFault::NoModelChosen)
            }
            other => (
                RefusalWire::Failed(other.sentence()),
                PlanFault::Unavailable,
            ),
        };
        if let Some(rt) = self.runtimes.get_mut(task) {
            rt.refused = Some(shown);
            rt.failure = Some(Failure::Model(fault));
        }
        vec![LoopInput::ModelFailed]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use companion_wire::footer_line;
    use porter_infer::{Declined, ModelRef};

    fn declined(model: ModelRef, because: DeclinedBecause) -> PlanFault {
        PlanFault::Declined(Box::new(Declined { model, because }))
    }

    #[test]
    fn only_a_decline_naming_an_agent_hands_the_turn_over() {
        let agent = ModelRef::agent("claude-code").expect("agent");
        let model = ModelRef {
            account: AccountId::parse("local").expect("account"),
            model: ModelId::parse("tiny").expect("model"),
        };
        let rows = [
            (
                "an agent chosen",
                declined(agent.clone(), DeclinedBecause::AgentChosen),
                Some("claude-code"),
            ),
            (
                "an agent the account named but nothing chose",
                declined(agent, DeclinedBecause::NotListed),
                None,
            ),
            (
                "a model that cannot serve",
                declined(model, DeclinedBecause::NotInstalled),
                None,
            ),
            ("no model", PlanFault::NoModelChosen, None),
        ];
        for (name, fault, expected) in rows {
            let got = chosen_agent(&fault);
            assert_eq!(got.as_ref().map(ProgramName::as_str), expected, "{name}");
        }
    }

    #[test]
    fn the_route_names_the_agent_the_way_the_person_does() {
        let program = ProgramName::parse("claude-code").expect("program");
        let line = footer_line(&route_of(&program, "Claude Code"));
        assert_eq!(line, "Answered by Claude Code (you chose it)");
    }
}
