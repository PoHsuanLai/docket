//! The planner over inferd: a chat session that sees the assembled view and answers with tool
//! calls or words. The planner is one resident model (QUESTIONS M2); the same model serves the
//! reader, the reviewer and the idle pass, scheduled around the person.

use agent_loop::ModelOutput;
use docket_core::PlannerView;
use porter_client::{AnyTransport, DbusTransport, Transport};
use porter_infer::ChatRequest;

/// Why the planner gave nothing usable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum PlanFault {
    /// No model could be reached or it refused.
    #[error("no planner model")]
    Unavailable,
    /// Its reply was not a tool call or words.
    #[error("unreadable reply")]
    Unreadable,
}

/// The planner model.
#[derive(Debug)]
pub struct PlannerModel<P: Transport> {
    infer: P,
}

impl PlannerModel<AnyTransport> {
    /// Asks inferd over the session bus (`AnyTransport::Dbus`). Nothing is called here: inferd is
    /// found, and started by activation, at the first session.
    pub fn on_bus(connection: &docket_dbus::BusConnection) -> Self {
        Self::new(AnyTransport::Dbus(DbusTransport::over(connection.clone())))
    }
}

impl<P: Transport> PlannerModel<P> {
    /// Asks through `infer`.
    pub fn new(infer: P) -> Self {
        Self { infer }
    }

    /// The chat request for a view: the action cards as tool declarations (`ToolDecl`), the
    /// sections in order, the clock only in the context section so the prefix stays cached.
    pub fn request(&self, view: &PlannerView) -> ChatRequest {
        let _ = (&self.infer, view);
        todo!(
            "PlannerModel::request: render the sections in the assembler's order into messages; ToolDecl per ActionCard; Usage::Interactive; a pinned session so the prefix cache is reused"
        )
    }

    /// One planner step.
    pub async fn plan(&self, view: &PlannerView) -> Result<ModelOutput, PlanFault> {
        let _ = (&self.infer, view);
        todo!(
            "PlannerModel::plan: Transport::open(Need::Llm with tools), send the request, read tool calls into PlannedCall (the tier from the action's app), text into Say"
        )
    }
}
