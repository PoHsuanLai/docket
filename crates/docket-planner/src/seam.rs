//! The planner as the task model sees it: the actions it may call, the voice it speaks in, and the
//! model turns it makes. [`PlannerModel`](crate::PlannerModel) is the one over a porter transport;
//! a test or an evaluation harness that has no transport uses the scripted one behind the
//! `testing` feature.

use crate::catalogue::Catalogue;
use crate::choices::ParamChoices;
use crate::planner::{PlanFault, PlannerReply};
use crate::role::RoleText;
use crate::summarise::{SummariseFault, Summarised};
use crate::voice::Voice;
use agent_loop::SummaryInput;
use docket_core::PlannerView;
use porter_infer::{ChatReply, ChatRequest};
use std::future::Future;

/// What drives a task: the catalogue and voice it is configured with, and the turns it asks.
pub trait Planner: Send + Sync {
    /// The actions offered.
    fn catalogue(&self) -> &Catalogue;

    /// Replaces the catalogue (an app was installed or removed).
    fn set_catalogue(&mut self, catalogue: Catalogue);

    /// The options apps offered for their dynamic parameters, as of now: the schema lists them
    /// until they are replaced. The caller asks the router; this crate reaches no bus.
    fn set_choices(&mut self, choices: Vec<ParamChoices>);

    /// The voice this planner speaks in now.
    fn voice(&self) -> Voice;

    /// Speaks in `voice` from now on: its role and the options its sessions open with.
    fn set_voice(&mut self, voice: Voice);

    /// Replaces the agent's role, for a task whose goal is only known once it opens.
    fn set_role(&mut self, role: RoleText);

    /// One planner step: what the model said, and what the loop does next.
    fn converse(
        &self,
        view: &PlannerView,
    ) -> impl Future<Output = Result<PlannerReply, PlanFault>> + Send;

    /// Runs a chat turn to its end and returns the reply (the idle pass's narratives).
    fn chat(
        &self,
        request: ChatRequest,
    ) -> impl Future<Output = Result<ChatReply, PlanFault>> + Send;

    /// The account of a task's oldest steps, from what the planner was shown alone.
    fn summarise(
        &self,
        input: &SummaryInput,
    ) -> impl Future<Output = Result<Summarised, SummariseFault>> + Send;
}
