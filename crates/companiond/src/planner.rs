//! The planner over inferd: a chat session that sees the assembled view and answers with tool
//! calls or words. The planner is one resident model (QUESTIONS M2); the same model serves the
//! reader, the reviewer and the idle pass, scheduled around the person.
//!
//! The request is a function of the view and the catalogue alone: the rules and the profile in
//! one system message, the rest in one user message in the assembler's order, the tools in the
//! registry's order. Nothing else goes in (no clock, no random ids), so two turns of one task
//! share every prompt token up to the first section that changed and the engine reuses its cache.

use crate::args::read_call;
use crate::catalogue::{Catalogue, CatalogueTool};
use crate::render::messages;
use agent_loop::{Availability, ModelOutput, Offer, PlannedCall, choose_tier};
use companion_wire::{RouteLog, RouteNote};
use docket_core::{CallRequest, Origin, PlannerView, ReaderAsk};
use docket_dbus::InferLink;
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Tier, Tokens};
use porter_infer::{
    ChatControl, ChatReply, ChatRequest, ClientFrame, Declined, InferEvent, InferReply,
    InferRequest, InferSession, JsonSchemaText, JsonText, Knob, Reasoning, ReplyShape, ServedBy,
    StopReason, ToolCallPart, ToolChoice, ToolDecl, ToolName, ToolParallelism,
};
use serde_json::{Value as Json, json};
use std::collections::BTreeSet;

/// The smallest context window the planner is asked for: the assembled view and room to answer.
const CONTEXT: Tokens = Tokens(12_000);

/// The tool that asks the person something.
pub const TOOL_ASK: &str = "quire_ask";
/// The tool that has the quarantined reader answer a question about handles.
pub const TOOL_READ: &str = "quire_read";
/// The tool that ends the task.
pub const TOOL_FINISH: &str = "quire_finish";

/// Why the planner gave nothing usable.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlanFault {
    /// No model could be reached or it refused.
    #[error("no planner model")]
    Unavailable,
    /// Its reply was not a tool call or words.
    #[error("unreadable reply")]
    Unreadable,
    /// The model the person named cannot serve; no other model answered in its place.
    #[error("the named model cannot serve")]
    Declined(Box<Declined>),
}

/// What the planner said and did in one turn: words first, then the one thing the loop acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerReply {
    /// Words for the person, if it spoke.
    pub said: Option<String>,
    /// What the loop does next: calls, a read, a question, or an end.
    pub then: ModelOutput,
    /// Who answered, for the answer's footer.
    pub served: Option<ServedBy>,
    /// How each stage of the turn was reached and why, for the footer line.
    pub route: Vec<RouteNote>,
}

/// The planner model.
#[derive(Debug)]
pub struct PlannerModel<P: Transport> {
    infer: P,
    catalogue: Catalogue,
}

impl PlannerModel<InferLink> {
    /// Asks inferd over the session bus (`InferLink`). Nothing is called here: inferd is
    /// found, and started by activation, at the first session.
    pub fn on_bus(connection: &docket_dbus::BusConnection) -> Self {
        Self::new(docket_dbus::inferd_transport(connection))
    }
}

fn schema_text(schema: &Json) -> Option<JsonSchemaText> {
    let text = serde_json::to_string(schema).ok()?;
    Some(JsonSchemaText(JsonText::parse(&text).ok()?))
}

fn meta_tool(name: &str, description: &str, params: &Json) -> Option<ToolDecl> {
    Some(ToolDecl {
        name: ToolName::parse(name).ok()?,
        description: description.to_owned(),
        params: schema_text(params)?,
    })
}

/// The three tools every task has beside the actions.
fn meta_tools() -> Vec<ToolDecl> {
    let ask = json!({
        "type": "object",
        "properties": {
            "text": { "type": "string", "maxLength": 400 },
            "choices": { "type": "array", "items": { "type": "string", "maxLength": 80 }, "maxItems": 6 },
        },
        "required": ["text"],
        "additionalProperties": false,
    });
    let read = json!({
        "type": "object",
        "properties": {
            "inputs": { "type": "array", "items": { "type": "integer", "minimum": 0 }, "minItems": 1 },
            "task": { "enum": ["classify", "extract", "summarise", "compare"] },
            "want": { "type": "object" },
        },
        "required": ["inputs", "task", "want"],
        "additionalProperties": false,
    });
    let finish = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    [
        meta_tool(TOOL_ASK, "Ask the person a question and wait for the answer.", &ask),
        meta_tool(
            TOOL_READ,
            "Have the reader answer a question about handles you cannot read. It answers in the shape you give.",
            &read,
        ),
        meta_tool(TOOL_FINISH, "The task is done.", &finish),
    ]
    .into_iter()
    .flatten()
    .collect()
}

/// The planner is deterministic enough to repeat: no sampling noise it does not need.
fn control() -> ChatControl {
    ChatControl {
        tool_choice: ToolChoice::Auto,
        tool_calls: ToolParallelism::Many,
        max_output: Knob::Off,
        reasoning: Reasoning::EngineDefault,
        sampling: Knob::Off,
        stop: Vec::new(),
    }
}

impl<P: Transport> PlannerModel<P> {
    /// Asks through `infer`, with no actions to offer until a catalogue is set.
    pub fn new(infer: P) -> Self {
        Self {
            infer,
            catalogue: Catalogue::default(),
        }
    }

    /// The actions this planner offers and reads calls by (the installed manifests).
    pub fn with_catalogue(self, catalogue: Catalogue) -> Self {
        Self { catalogue, ..self }
    }

    /// Replaces the catalogue (an app was installed or removed).
    pub fn set_catalogue(&mut self, catalogue: Catalogue) {
        self.catalogue = catalogue;
    }

    /// The actions offered.
    pub fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    /// The tools of a view: the actions the budget kept, in the view's order, then the three
    /// every task has.
    fn tools(&self, view: &PlannerView) -> Vec<ToolDecl> {
        view.actions
            .iter()
            .filter_map(|card| self.catalogue.of(&card.action))
            .filter_map(CatalogueTool::declaration)
            .chain(meta_tools())
            .collect()
    }

    /// The chat request for a view: the sections in the assembler's order, the action cards as
    /// tool declarations (`ToolDecl`), nothing that changes between turns but the content.
    pub fn request(&self, view: &PlannerView) -> ChatRequest {
        self.request_for(view, Usage::Interactive)
    }

    fn request_for(&self, view: &PlannerView, usage: Usage) -> ChatRequest {
        ChatRequest {
            messages: messages(view),
            shape: ReplyShape::Text,
            tier: Tier::Balanced,
            class: DataClass::Prompt,
            usage,
            tools: self.tools(view),
            control: control(),
        }
    }

    /// One planner step: what the model said, and what the loop does next.
    pub async fn converse(&self, view: &PlannerView) -> Result<PlannerReply, PlanFault> {
        let (reply, route) = self.chat_routed(self.request(view)).await?;
        let mut said = self.read(reply)?;
        said.route = route;
        Ok(said)
    }

    /// One planner step as the loop's single output: calls, a read, a question, an end, or
    /// words when the model only spoke.
    pub async fn plan(&self, view: &PlannerView) -> Result<ModelOutput, PlanFault> {
        let reply = self.converse(view).await?;
        Ok(match (reply.said, reply.then) {
            (Some(words), ModelOutput::Finish) => ModelOutput::Say(words),
            (_, then) => then,
        })
    }

    /// Runs a chat turn to its end and returns the reply.
    pub(crate) async fn chat(&self, request: ChatRequest) -> Result<ChatReply, PlanFault> {
        self.chat_routed(request).await.map(|(reply, _)| reply)
    }

    /// A chat turn and how it was routed: the reasons, doors and stages inferd announced.
    pub(crate) async fn chat_routed(
        &self,
        request: ChatRequest,
    ) -> Result<(ChatReply, Vec<RouteNote>), PlanFault> {
        let need = Need::Llm(LlmNeed {
            features: BTreeSet::from([LlmFeature::Chat, LlmFeature::Tools]),
            context: CONTEXT,
        });
        let (class, tier) = (request.class, request.tier);
        let mut session: AnyOrP<P> = self
            .infer
            .open(&need, class, tier)
            .await
            .map_err(|_| PlanFault::Unavailable)?;
        session
            .send(ClientFrame::Request(InferRequest::Chat(request)))
            .await
            .map_err(|_| PlanFault::Unavailable)?;
        let mut route = RouteLog::default();
        loop {
            let event = session.next().await.map_err(|_| PlanFault::Unavailable)?;
            route.event(&event);
            match event {
                InferEvent::Finished(InferReply::Chat(reply)) => return Ok((reply, route.notes())),
                InferEvent::Finished(_) => {
                    return Err(match route.declined() {
                        Some(declined) => PlanFault::Declined(Box::new(declined.clone())),
                        None => PlanFault::Unavailable,
                    });
                }
                _ => {}
            }
        }
    }

    /// The reply as the loop's output.
    fn read(&self, reply: ChatReply) -> Result<PlannerReply, PlanFault> {
        if matches!(
            reply.stop,
            StopReason::MaxTokens | StopReason::ContentFilter
        ) {
            return Err(PlanFault::Unreadable);
        }
        let said = Some(reply.text.trim().to_owned()).filter(|t| !t.is_empty());
        let (actions, meta): (Vec<_>, Vec<_>) = reply
            .tool_calls
            .iter()
            .partition(|c| !is_meta(c.name.as_str()));
        let calls: Vec<PlannedCall> = actions.iter().filter_map(|c| self.planned(c)).collect();
        let then = if !calls.is_empty() {
            ModelOutput::Calls(calls)
        } else if !actions.is_empty() {
            return Err(PlanFault::Unreadable);
        } else {
            match meta.first() {
                Some(call) => meta_output(call)?,
                None if said.is_some() => ModelOutput::Finish,
                None => return Err(PlanFault::Unreadable),
            }
        };
        Ok(PlannerReply {
            said,
            then,
            served: Some(reply.served),
            route: Vec::new(),
        })
    }

    /// One action call, if the model named a tool of the catalogue and its arguments fit.
    fn planned(&self, call: &ToolCallPart) -> Option<PlannedCall> {
        let tool = self.catalogue.named(call.name.as_str())?;
        let json: Json = serde_json::from_str(call.args.as_str()).ok()?;
        let read = read_call(tool, &json).ok()?;
        let tier = choose_tier(&Availability {
            typed: Offer::Offered,
            hook: Offer::Absent,
            cua: Offer::Absent,
        })?;
        Some(PlannedCall {
            call: CallRequest {
                action: tool.action(),
                target: read.target,
                args: read.args,
                origin: Origin::Companion,
            },
            tier,
        })
    }
}

/// The session type of a transport.
type AnyOrP<P> = <P as Transport>::Session;

fn is_meta(name: &str) -> bool {
    matches!(name, TOOL_ASK | TOOL_READ | TOOL_FINISH)
}

fn meta_output(call: &ToolCallPart) -> Result<ModelOutput, PlanFault> {
    let args: Json = serde_json::from_str(call.args.as_str()).map_err(|_| PlanFault::Unreadable)?;
    match call.name.as_str() {
        TOOL_FINISH => Ok(ModelOutput::Finish),
        TOOL_ASK => {
            let text = args
                .get("text")
                .and_then(Json::as_str)
                .ok_or(PlanFault::Unreadable)?
                .to_owned();
            let choices = args
                .get("choices")
                .and_then(Json::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|c| c.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            Ok(ModelOutput::Ask { text, choices })
        }
        _ => serde_json::from_value::<ReaderAsk>(args)
            .map(ModelOutput::Read)
            .map_err(|_| PlanFault::Unreadable),
    }
}
