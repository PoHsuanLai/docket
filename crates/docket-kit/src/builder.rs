//! The builder: every choice is a typed value, and `build` checks them once.

use crate::actions::Actions;
use crate::agent::Agent;
use crate::fault::BuildFault;
use crate::limits::Limits;
use crate::memory::Memory;
use docket_client::{Intents, Transport as IntentsTransport};
use docket_core::{AgentConfig, AssemblerBudget};
use docket_planner::{PlannerModel, RoleFault, RoleText};
use porter_client::Transport as InferTransport;
use porter_core::Tier;

/// Declares an agent. Everything has a default that is the safe one: no actions, no memory, the
/// `Balanced` tier, the assembler's budgets, eight model turns. The fixed rules of the planner
/// (`docket_planner::RULES`) always come first in the system message; the role follows them and
/// cannot remove them.
///
/// ```no_run
/// use docket_kit::{
///     Actions, Agent, Asker, Catalogue, Count, InferTransport, Intents, IntentsTransport, Limits,
///     Memory, RecallScope, Tier, TrustFilter,
/// };
///
/// # async fn demo<P: InferTransport, I: IntentsTransport>(
/// #     infer: P,
/// #     intents: Intents<I>,
/// #     catalogue: Catalogue,
/// #     asker: Asker,
/// # ) -> Result<(), Box<dyn std::error::Error>> {
/// let agent = Agent::builder(infer, intents)
///     .role("You review outgoing changes for anything the person did not mean to make.")
///     .actions(Actions::from(&catalogue).app("org.quire.Notes").reads_only())
///     .memory(Memory::none().recall(RecallScope::AskedSpace, TrustFilter::TrustedOnly))
///     .tier(Tier::Balanced)
///     .limits(Limits { steps: Count(6) })
///     .build()?;
/// let run = agent.ask(&asker, "Review the draft I just made.").await?;
/// println!("{:?}: {} steps", run.ended, run.steps.len());
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct AgentBuilder<P: InferTransport, I: IntentsTransport> {
    infer: P,
    intents: Intents<I>,
    role: Option<Result<RoleText, RoleFault>>,
    actions: Actions,
    memory: Memory,
    tier: Tier,
    budget: AssemblerBudget,
    limits: Limits,
}

impl<P: InferTransport, I: IntentsTransport> AgentBuilder<P, I> {
    pub(crate) fn new(infer: P, intents: Intents<I>) -> Self {
        Self {
            infer,
            intents,
            role: None,
            actions: Actions::none(),
            memory: Memory::none(),
            tier: Tier::Balanced,
            budget: AgentConfig::default().assembler,
            limits: Limits::default(),
        }
    }

    /// What this agent is for, after the fixed rules. A text that is empty or too long is a
    /// fault of `build`.
    pub fn role(self, text: &str) -> Self {
        Self {
            role: Some(RoleText::parse(text)),
            ..self
        }
    }

    /// The actions it may be offered (see [`Actions`]); none by default.
    pub fn actions(self, actions: Actions) -> Self {
        Self { actions, ..self }
    }

    /// The memory sections it sees; none by default.
    pub fn memory(self, memory: Memory) -> Self {
        Self { memory, ..self }
    }

    /// The kind of model it asks for.
    pub fn tier(self, tier: Tier) -> Self {
        Self { tier, ..self }
    }

    /// The assembler's budgets, when the defaults (`AgentConfig::assembler`) do not fit.
    pub fn budget(self, budget: AssemblerBudget) -> Self {
        Self { budget, ..self }
    }

    /// How far one ask may go.
    pub fn limits(self, limits: Limits) -> Self {
        Self { limits, ..self }
    }

    /// Checks the choices and makes the agent.
    pub fn build(self) -> Result<Agent<P, I>, BuildFault> {
        if let Some(missing) = self.actions.missing().first() {
            return Err(BuildFault::Missing(missing.clone()));
        }
        let planner = PlannerModel::new(self.infer)
            .with_catalogue(self.actions.catalogue())
            .with_tier(self.tier);
        let planner = match self.role.transpose()? {
            Some(role) => planner.with_role(role),
            None => planner,
        };
        Ok(Agent {
            intents: self.intents,
            planner,
            memory: self.memory,
            budget: self.budget,
            limits: self.limits,
        })
    }
}
