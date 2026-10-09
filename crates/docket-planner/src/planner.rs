//! The planner over inferd: a chat session that sees the assembled view and answers with tool
//! calls or words. The planner is one resident model (QUESTIONS M2); the same model serves the
//! reader, the reviewer and the idle pass, scheduled around the person.
//!
//! The request is a function of the view and the catalogue alone: the rules and the profile in
//! one system message, the rest in one user message in the assembler's order, the tools in the
//! registry's order. Nothing else goes in (no clock, no random ids), so two turns of one task
//! share every prompt token up to the first section that changed and the engine reuses its cache.

use crate::args::{ArgsFault, read_call};
use crate::catalogue::{Catalogue, CatalogueTool};
use crate::read_ask::read_output;
use crate::render::messages_with;
use crate::role::RoleText;
use crate::want::want_schema;
use agent_loop::{Availability, ModelOutput, Offer, PlannedCall, choose_tier, leaked_call};
use companion_wire::{RouteLog, RouteNote};
use docket_core::{
    ActionCard, Args, CallRequest, Handle, HandleShape, Origin, ParamName, PlannerView,
    RELATION_ARG, ReplyFault, TargetValue, Value, Why,
};
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{AppName, DataClass, Need, Tier, Tokens};
use porter_infer::{
    ChatControl, ChatReply, ChatRequest, ClientFrame, Declined, InferEvent, InferReply,
    InferRequest, InferSession, JsonSchemaText, JsonText, Knob, Reasoning, ReplyShape, ServedBy,
    StopReason, ToolCallPart, ToolChoice, ToolDecl, ToolName, ToolParallelism,
};
use prov::{EntityId, EntityKey, EntityKind};
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
/// The tool that asks for the thing related to a thing it holds (the sender of a thread), as a
/// handle. Offered only when some app declares a relation.
pub const TOOL_RELATED: &str = "quire_related";

/// Why the planner gave nothing usable.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum PlanFault {
    /// No model could be reached or it refused.
    #[error("no planner model")]
    Unavailable,
    /// Its reply was not a tool call or words.
    #[error("unreadable reply")]
    Unreadable,
    /// The model wrote a tool call as words and made none: its server's tool parser did not
    /// read it. Nothing was done, and the words are not shown.
    #[error("a call written as words")]
    CallInText,
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
    role: Option<RoleText>,
    tier: Tier,
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

/// The relations the view's actions resolve, each name once, in the view's order.
fn relation_names(actions: &[ActionCard]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for relation in actions.iter().flat_map(|card| &card.related) {
        if !names.iter().any(|n| n == relation.name.as_str()) {
            names.push(relation.name.as_str().to_owned());
        }
    }
    names
}

/// `quire_related`, when some action of the view resolves a relation.
fn related_tool(actions: &[ActionCard]) -> Option<ToolDecl> {
    let names = relation_names(actions);
    if names.is_empty() {
        return None;
    }
    let params = json!({
        "type": "object",
        "properties": {
            "of": {
                "anyOf": [
                    {
                        "type": "object",
                        "properties": { "handle": { "type": "integer", "minimum": 0 } },
                        "required": ["handle"],
                        "additionalProperties": false,
                    },
                    {
                        "type": "object",
                        "properties": {
                            "app": { "type": "string" },
                            "kind": { "type": "string" },
                            "key": { "type": "string" },
                        },
                        "required": ["app", "kind", "key"],
                        "additionalProperties": false,
                    },
                ],
            },
            "relation": { "enum": names },
        },
        "required": ["of", "relation"],
        "additionalProperties": false,
    });
    meta_tool(
        TOOL_RELATED,
        "Get a thing related to a thing you hold, as a handle: \"of\" is the thing, as {\"handle\": n} or as {\"app\", \"kind\", \"key\"}, and \"relation\" is one of the relations shown beside it (\"related: ...\"). Use it when an action needs a thing of another kind that you cannot read from the one you hold, such as who sent a thread.",
        &params,
    )
}

/// The tools every task has beside the actions (`quire_related` first, where it is offered, so
/// the last three are always ask, read and finish).
fn meta_tools(actions: &[ActionCard]) -> Vec<ToolDecl> {
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
            "want": want_schema(),
        },
        "required": ["inputs", "task", "want"],
        "additionalProperties": false,
    });
    let finish = json!({ "type": "object", "properties": {}, "additionalProperties": false });
    [
        related_tool(actions),
        meta_tool(TOOL_ASK, "Ask the person a question and wait for the answer.", &ask),
        meta_tool(
            TOOL_READ,
            "Have the reader answer a question about handles you cannot read. \"want\" is the shape of the answer, as {\"kind\": \"choice\", \"v\": [\"yes\", \"no\"]} (kinds: choice, integer, date, datetime, text, record, list); it is not a JSON Schema. A choice, integer, date or datetime answer is returned to you to read; an answer in words (text, or a record or list holding text) is returned as a handle you cannot read, so use it only as an argument.",
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
            role: None,
            tier: Tier::Balanced,
        }
    }

    /// The same planner with an agent's role after the fixed rules.
    pub fn with_role(self, role: RoleText) -> Self {
        Self {
            role: Some(role),
            ..self
        }
    }

    /// The same planner asking for `tier` of model (the companion's is `Balanced`).
    pub fn with_tier(self, tier: Tier) -> Self {
        Self { tier, ..self }
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
            .filter(|card| card.related.is_empty())
            .filter_map(|card| self.catalogue.of(&card.action))
            .filter_map(CatalogueTool::declaration)
            .chain(meta_tools(&view.actions))
            .collect()
    }

    /// The chat request for a view: the sections in the assembler's order, the action cards as
    /// tool declarations (`ToolDecl`), nothing that changes between turns but the content.
    pub fn request(&self, view: &PlannerView) -> ChatRequest {
        self.request_for(view, Usage::Interactive)
    }

    fn request_for(&self, view: &PlannerView, usage: Usage) -> ChatRequest {
        ChatRequest {
            messages: messages_with(view, self.role.as_ref()),
            shape: ReplyShape::Text,
            tier: self.tier,
            class: DataClass::Prompt,
            usage,
            tools: self.tools(view),
            control: control(),
        }
    }

    /// One planner step: what the model said, and what the loop does next.
    pub async fn converse(&self, view: &PlannerView) -> Result<PlannerReply, PlanFault> {
        let (reply, route) = self.chat_routed(self.request(view)).await?;
        let mut said = self.read(reply, view)?;
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
    pub async fn chat(&self, request: ChatRequest) -> Result<ChatReply, PlanFault> {
        self.chat_routed(request).await.map(|(reply, _)| reply)
    }

    /// A chat turn and how it was routed: the reasons, doors and stages inferd announced.
    pub async fn chat_routed(
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
    fn read(&self, reply: ChatReply, view: &PlannerView) -> Result<PlannerReply, PlanFault> {
        if matches!(
            reply.stop,
            StopReason::MaxTokens | StopReason::ContentFilter
        ) {
            return Err(PlanFault::Unreadable);
        }
        let said = Some(bounded(reply.text.trim())).filter(|t| !t.is_empty());
        // A call left in the words is never a call, and its markup is not an answer: with real
        // calls beside it the words are dropped, alone the turn fails with a name for it.
        let said = match (said, reply.tool_calls.is_empty()) {
            (Some(words), true) if leaked_call(&words).is_some() => {
                return Err(PlanFault::CallInText);
            }
            (Some(words), false) if leaked_call(&words).is_some() => None,
            (said, _) => said,
        };
        let (actions, meta): (Vec<_>, Vec<_>) = reply
            .tool_calls
            .iter()
            .partition(|c| !is_meta(c.name.as_str()));
        let (calls, faults): (Vec<_>, Vec<_>) = actions
            .iter()
            .map(|c| self.planned(c))
            .partition(Result::is_ok);
        let calls: Vec<PlannedCall> = calls.into_iter().flatten().collect();
        let then = if !calls.is_empty() {
            ModelOutput::Calls(calls)
        } else if let Some(Err(fault)) = faults.into_iter().next() {
            // Nothing to run: the model is told what was wrong with its first call.
            ModelOutput::Unread(fault)
        } else {
            match meta.first() {
                Some(call) => meta_output(call, view)?,
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

    /// One action call, if the model named a tool of the catalogue and its arguments fit; else
    /// what was wrong with it.
    fn planned(&self, call: &ToolCallPart) -> Result<PlannedCall, ReplyFault> {
        let tool = self
            .catalogue
            .named(call.name.as_str())
            .ok_or_else(|| ReplyFault::NoSuchTool(plain_name(call.name.as_str())))?;
        let json: Json =
            serde_json::from_str(call.args.as_str()).map_err(|_| ReplyFault::NotJson)?;
        let read = read_call(tool, &json).map_err(|fault| ReplyFault::Args(plain_fault(fault)))?;
        let tier = choose_tier(&Availability {
            typed: Offer::Offered,
            hook: Offer::Absent,
            cua: Offer::Absent,
        })
        .ok_or(ReplyFault::NoSuchTool(String::new()))?;
        Ok(PlannedCall {
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

/// A name the model wrote, as far as it is plain enough to say back: letters, digits and `._-`,
/// at most 64 of them; anything else is not repeated.
fn plain_name(name: &str) -> String {
    let plain = name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    if plain {
        name.to_owned()
    } else {
        String::new()
    }
}

/// An argument fault that names nothing the model wrote but a plain name.
fn plain_fault(fault: ArgsFault) -> ArgsFault {
    match fault {
        ArgsFault::Unknown(name) => ArgsFault::Unknown(plain_name(&name)),
        other => other,
    }
}

/// The most words of one reply that are kept: a model with no output limit can say a great deal,
/// and the person's answer, the history and the bus carry what is kept.
const MOST_WORDS: usize = 32_000;

/// `text` without the marks that reorder or hide words: the model's words are drawn as written,
/// never reversed or concealed.
fn plain(text: &str) -> String {
    text.chars()
        .filter(|c| !docket_core::reorders(*c) && !docket_core::hides(*c))
        .collect()
}

/// `text` as the person is shown it: plain, and cut at [`MOST_WORDS`] characters with an
/// ellipsis where it was cut.
fn bounded(text: &str) -> String {
    let shown = plain(text);
    match shown.char_indices().nth(MOST_WORDS) {
        Some((end, _)) => format!("{}\u{2026}", &shown[..end]),
        None => shown,
    }
}

/// The session type of a transport.
type AnyOrP<P> = <P as Transport>::Session;

fn is_meta(name: &str) -> bool {
    matches!(name, TOOL_ASK | TOOL_READ | TOOL_FINISH | TOOL_RELATED)
}

fn meta_output(call: &ToolCallPart, view: &PlannerView) -> Result<ModelOutput, PlanFault> {
    let Ok(args) = serde_json::from_str::<Json>(call.args.as_str()) else {
        return match call.name.as_str() {
            TOOL_READ | TOOL_RELATED => Ok(ModelOutput::Unread(ReplyFault::NotJson)),
            _ => Err(PlanFault::Unreadable),
        };
    };
    match call.name.as_str() {
        TOOL_FINISH => Ok(ModelOutput::Finish),
        TOOL_ASK => question(&args),
        TOOL_RELATED => Ok(related_output(&args, view)),
        _ => Ok(read_output(&args)),
    }
}

fn param(name: &str) -> Option<ParamName> {
    ParamName::parse(name).ok()
}

fn wrong(name: &str, why: Why) -> ModelOutput {
    match param(name) {
        Some(param) => ModelOutput::Unread(ReplyFault::Args(ArgsFault::Wrong { param, why })),
        None => ModelOutput::Unread(ReplyFault::NotJson),
    }
}

fn missing(name: &str) -> ModelOutput {
    match param(name) {
        Some(param) => ModelOutput::Unread(ReplyFault::Args(ArgsFault::Missing(param))),
        None => ModelOutput::Unread(ReplyFault::NotJson),
    }
}

/// A `quire_related` as the call of the related action of the thing's kind: the same call the
/// router gates and records as any other read, so a planner gets a related thing no other way
/// than through the gate. The thing is a handle the view shows or a thing named as `target`
/// names it (`{"app", "kind", "key"}`, as the context shows the thing the person has open), and
/// its kind must have that relation.
fn related_output(args: &Json, view: &PlannerView) -> ModelOutput {
    let Some(of) = args.get("of") else {
        return missing("of");
    };
    let Some(relation) = args.get("relation").and_then(Json::as_str) else {
        return missing(RELATION_ARG);
    };
    let Some((target, kind)) = related_subject(of, view) else {
        return wrong("of", Why::Type);
    };
    let kind = &kind;
    let Some(card) = view.actions.iter().find(|card| {
        matches!(&card.on, docket_core::TargetKind::One(k) if k == kind)
            && card.related.iter().any(|r| r.name.as_str() == relation)
    }) else {
        return wrong(RELATION_ARG, Why::Range);
    };
    let (Some(name), Some(choice)) = (
        param(RELATION_ARG),
        docket_core::ChoiceId::parse(relation).ok(),
    ) else {
        return wrong(RELATION_ARG, Why::Range);
    };
    let Some(tier) = choose_tier(&Availability {
        typed: Offer::Offered,
        hook: Offer::Absent,
        cua: Offer::Absent,
    }) else {
        return ModelOutput::Unread(ReplyFault::NoSuchTool(String::new()));
    };
    let mut args = Args::new();
    args.insert(
        name,
        prov::Labelled {
            value: Value::Choice(choice),
            label: crate::args::planner_label(),
        },
    );
    ModelOutput::Calls(vec![PlannedCall {
        call: CallRequest {
            action: card.action.clone(),
            target,
            args,
            origin: Origin::Companion,
        },
        tier,
    }])
}

/// The longest question the planner may put to the person, and its choices: the limits the tool's
/// schema states, which a model is not made to keep.
const ASK_TEXT: usize = 400;
const ASK_CHOICES: usize = 6;
const ASK_CHOICE_TEXT: usize = 80;

/// A question as the schema allows it: bounded text and at most six bounded choices, every one a
/// string. Anything else is an unreadable reply, not a question trimmed to fit.
fn question(args: &Json) -> Result<ModelOutput, PlanFault> {
    let text = args
        .get("text")
        .and_then(Json::as_str)
        .filter(|t| t.chars().count() <= ASK_TEXT)
        .map(plain)
        .filter(|t| !t.trim().is_empty())
        .ok_or(PlanFault::Unreadable)?;
    let choices = match args.get("choices") {
        None => Vec::new(),
        Some(Json::Array(items)) if items.len() <= ASK_CHOICES => items
            .iter()
            .map(|c| {
                c.as_str()
                    .filter(|c| c.chars().count() <= ASK_CHOICE_TEXT)
                    .map(plain)
                    .ok_or(PlanFault::Unreadable)
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err(PlanFault::Unreadable),
    };
    Ok(ModelOutput::Ask { text, choices })
}

/// The thing a `quire_related` is about and its kind: a handle the view holds a thing for, or a
/// thing named in full.
fn related_subject(of: &Json, view: &PlannerView) -> Option<(TargetValue, EntityKind)> {
    let object = of.as_object()?;
    if let (1, Some(n)) = (object.len(), object.get("handle")) {
        let held = Handle(n.as_u64()?);
        let kind = view.handles.iter().find_map(|c| match &c.shape {
            HandleShape::Entity(kind) if c.handle == held => Some(kind.clone()),
            _ => None,
        })?;
        return Some((TargetValue::Handles(vec![held]), kind));
    }
    let text = |name: &str| object.get(name).and_then(Json::as_str);
    let id = EntityId {
        app: AppName::parse(text("app")?).ok()?,
        kind: EntityKind::parse(text("kind")?).ok()?,
        key: EntityKey::parse(text("key")?).ok()?,
    };
    (object.len() == 3).then(|| (TargetValue::Entities(vec![id.clone()]), id.kind))
}
