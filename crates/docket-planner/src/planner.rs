//! The planner over inferd: a chat session that sees the assembled view and answers with tool
//! calls or words. The planner is one resident model (QUESTIONS M2); the same model serves the
//! reader, the reviewer and the idle pass, scheduled around the person.
//!
//! The request is a function of the view and the catalogue alone: the rules and the profile in
//! one system message, the rest in one user message in the assembler's order, the tools in the
//! registry's order. Nothing else goes in (no clock, no random ids), so two turns of one task
//! share every prompt token up to the first section that changed and the engine reuses its cache.

use crate::args::read_call;
use crate::catalogue::Catalogue;
use crate::choices::ParamChoices;
use crate::known::known_values;
use crate::meta_reply::{
    answer_text, bounded, is_meta, meta_output, outside_known, plain_fault, plain_name,
};
use crate::meta_tools::{control, meta_tools};
use crate::plain::without_ids;
use crate::render::messages_with;
use crate::role::RoleText;
use crate::seam::Planner;
use crate::show::finish_output;
use crate::summarise::{SummariseFault, Summarised};
use crate::voice::Voice;
use agent_loop::{
    Availability, ModelOutput, Offer, PlannedCall, SummaryInput, choose_tier, leaked_call,
};
use companion_wire::{RouteLog, RouteNote};
use docket_core::{ActionRef, CallRequest, Handle, OneOf, Origin, PlannerView, ReplyFault};
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Tier, Tokens};
use porter_infer::{
    ChatReply, ChatRequest, ClientFrame, Declined, InferEvent, InferRefusal, InferReply,
    InferRequest, InferSession, OpenOptions, ReplyShape, ServedBy, StopReason, ToolCallPart,
    ToolDecl,
};
use serde_json::Value as Json;
use std::collections::BTreeSet;

/// The smallest context window the planner is asked for: the assembled view and room to answer.
const CONTEXT: Tokens = Tokens(12_000);

/// The tool that asks the person something.
pub const TOOL_ASK: &str = "quire_ask";
/// The tool that has the quarantined reader answer a question about handles.
pub const TOOL_READ: &str = "quire_read";
/// The tool that gives the person the reply and ends the turn.
pub const TOOL_ANSWER: &str = "quire_answer";
/// The tool that ends the task.
pub const TOOL_FINISH: &str = "quire_finish";
/// The tool that asks for the thing related to a thing it holds (the sender of a thread), as a
/// handle. Offered only when some app declares a relation.
pub const TOOL_RELATED: &str = "quire_related";

/// Why the planner gave nothing usable.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[non_exhaustive]
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
    /// The person has not chosen a model for the assistant, and none is chosen for them.
    #[error("no model is chosen")]
    NoModelChosen,
    /// The person's choice is Automatic and none of its models showed it can use apps.
    #[error("no model in Automatic can use apps")]
    NoModelUsesApps,
}

/// What the planner said and did in one turn: words first, then the one thing the loop acts on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerReply {
    /// Words for the person, if it spoke.
    pub said: Option<String>,
    /// What the loop does next: calls, a read, a question, or an end.
    pub then: ModelOutput,
    /// The text handles the person is shown in the answer, as `quire_finish` named them: each
    /// one a text handle of the view, once. The planner never reads them.
    pub show: Vec<Handle>,
    /// Who answered, for the answer's footer.
    pub served: Option<ServedBy>,
    /// How each stage of the turn was reached and why, for the footer line.
    pub route: Vec<RouteNote>,
}

/// The planner model.
///
/// Over a model that only talks, a turn that makes no call ends in the words:
///
/// ```
/// # #[tokio::main(flavor = "current_thread")]
/// # async fn main() -> Result<(), Box<dyn std::error::Error>> {
/// use agent_loop::{ModelOutput, Sources, assemble};
/// use docket_core::*;
/// use docket_fake::WordsModel;
/// use docket_planner::PlannerModel;
/// use porter_core::{AppName, Count};
/// use prov::Integrity;
///
/// let view = assemble(
///     &AgentConfig::default().assembler,
///     &Sources {
///         cards: vec![],
///         profile: vec![],
///         primer: None,
///         rollup: None,
///         roster: Roster::default(),
///         episodes: vec![],
///         recalled: vec![],
///         context: ContextView {
///             app: AppName::parse("org.quire.Shell")?,
///             window: Reveal::Plain(String::new()),
///             here: HereView::Nowhere,
///             selection: SelectionView::Nothing,
///             visible: VisibleView {
///                 kind: None,
///                 items: vec![],
///                 total: Count(0),
///             },
///             text_target: TextTargetView::None,
///         },
///         turns: vec![],
///         history: vec![],
///         handles: vec![],
///         inbox: vec![],
///         skills: vec![],
///         skill_texts: vec![],
///         shown: vec![],
///         taint: Integrity::Trusted,
///         task_policy: None,
///         summary: None,
///     },
/// );
/// let planner = PlannerModel::new(WordsModel::says("Nothing to do."));
/// assert_eq!(
///     planner.plan(&view).await?,
///     ModelOutput::Say("Nothing to do.".into())
/// );
/// # Ok(()) }
/// ```
#[derive(Debug)]
pub struct PlannerModel<P: Transport> {
    infer: P,
    catalogue: Catalogue,
    choices: Vec<ParamChoices>,
    pub(crate) role: Option<RoleText>,
    pub(crate) tier: Tier,
    options: OpenOptions,
}

impl<P: Transport> PlannerModel<P> {
    /// Asks through `infer`, with no actions to offer until a catalogue is set.
    pub fn new(infer: P) -> Self {
        Self {
            infer,
            catalogue: Catalogue::default(),
            choices: Vec::new(),
            role: None,
            tier: Tier::Balanced,
            options: OpenOptions::default(),
        }
    }

    /// The same planner opening its inference sessions with `options`: who asks (a helper), and
    /// the one model it must run on. The assistant's own planner opens with none.
    pub fn with_open_options(self, options: OpenOptions) -> Self {
        Self { options, ..self }
    }

    /// The options every inference session of this planner opens with.
    pub fn open_options(&self) -> &OpenOptions {
        &self.options
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

    /// What this turn lists for `action`'s parameters: the view's known values, then the
    /// options its app offered.
    fn listed(&self, action: &ActionRef, view: &PlannerView) -> Vec<OneOf> {
        let offered = self.choices.iter().filter(|c| c.is_of(action));
        known_values(action, view)
            .into_iter()
            .chain(offered.map(ParamChoices::one_of))
            .collect()
    }

    /// The tools of a view: the actions the budget kept, in the view's order, then the three
    /// every task has.
    pub(crate) fn tools(&self, view: &PlannerView) -> Vec<ToolDecl> {
        view.actions
            .iter()
            .filter(|card| card.related.is_empty())
            .filter_map(|card| self.catalogue.of(&card.action))
            .filter_map(|tool| {
                tool.declaration_for(&view.handles, &self.listed(&tool.action(), view))
            })
            .chain(meta_tools(&view.actions))
            .collect()
    }

    /// The chat request for a view: the sections in the assembler's order, the action cards as
    /// tool declarations (`ToolDecl`), nothing that changes between turns but the content.
    pub fn request(&self, view: &PlannerView) -> ChatRequest {
        self.request_for(view, Usage::Interactive)
    }

    fn request_for(&self, view: &PlannerView, usage: Usage) -> ChatRequest {
        ChatRequest::new(
            messages_with(view, self.role.as_ref()),
            self.tier,
            DataClass::Prompt,
            usage,
        )
        .with_shape(ReplyShape::Text)
        .with_tools(self.tools(view))
        .with_control(control())
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

    /// A chat turn and how it was routed: the reasons, doors and stages inferd announced.
    pub async fn chat_routed(
        &self,
        request: ChatRequest,
    ) -> Result<(ChatReply, Vec<RouteNote>), PlanFault> {
        let need = Need::Llm(LlmNeed::new(
            BTreeSet::from([LlmFeature::Chat, LlmFeature::Tools]),
            CONTEXT,
        ));
        let (class, tier) = (request.class, request.tier);
        let mut session: AnyOrP<P> = self
            .infer
            .open_with(&need, class, tier, &self.options)
            .await
            .map_err(|e| unavailable("opening a model", &e))?;
        session
            .send(ClientFrame::Request(InferRequest::Chat(request)))
            .await
            .map_err(|e| unavailable("sending the request", &e))?;
        let mut route = RouteLog::default();
        loop {
            let event = session
                .next()
                .await
                .map_err(|e| unavailable("reading the reply", &e))?;
            route.event(&event);
            match event {
                InferEvent::Finished(InferReply::Chat(reply)) => return Ok((reply, route.notes())),
                InferEvent::Finished(other) => {
                    tracing::warn!(
                        got = not_chat(&other),
                        "the planner's model finished without a chat reply"
                    );
                    return Err(match (&other, route.declined()) {
                        (InferReply::Refused(InferRefusal::NoModelChosen), _) => {
                            PlanFault::NoModelChosen
                        }
                        (InferReply::Refused(InferRefusal::NoModelUsesApps), _) => {
                            PlanFault::NoModelUsesApps
                        }
                        (_, Some(declined)) => PlanFault::Declined(Box::new(declined.clone())),
                        (_, None) => PlanFault::Unavailable,
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
            tracing::warn!(stop = ?reply.stop, "the planner's reply stopped early");
            return Err(PlanFault::Unreadable);
        }
        // The person never reads an id the model repeated from its view ("t-179"): said as what it is.
        let mut said = Some(without_ids(&bounded(reply.text.trim()))).filter(|t| !t.is_empty());
        // A call left in the words is never a call, and its markup is not an answer: with real
        // calls beside it the words are dropped, alone the turn fails with a name for it.
        said = match (said, reply.tool_calls.is_empty()) {
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
            .map(|c| self.planned(c, view))
            .partition(Result::is_ok);
        let calls: Vec<PlannedCall> = calls.into_iter().flatten().collect();
        let (then, show) = if !calls.is_empty() {
            (ModelOutput::Calls(calls), Vec::new())
        } else if let Some(Err(fault)) = faults.into_iter().next() {
            // Nothing to run: the model is told what was wrong with its first call.
            (ModelOutput::Unread(fault), Vec::new())
        } else {
            match meta.first() {
                Some(call) if call.name.as_str() == TOOL_FINISH => finish_output(call, view)?,
                Some(call) if call.name.as_str() == TOOL_ANSWER => {
                    // The reply is the words: whatever else the model wrote beside it was not.
                    let reply = answer_text(call)?;
                    said = Some(reply);
                    (ModelOutput::Finish, Vec::new())
                }
                Some(call) => (meta_output(call, view)?, Vec::new()),
                None if said.is_some() => (ModelOutput::Finish, Vec::new()),
                None => {
                    tracing::warn!("the planner's reply had no words and no call");
                    return Err(PlanFault::Unreadable);
                }
            }
        };
        Ok(PlannerReply {
            said,
            then,
            show,
            served: Some(reply.served),
            route: Vec::new(),
        })
    }

    /// One action call, if the model named a tool of the catalogue and its arguments fit; else
    /// what was wrong with it.
    fn planned(&self, call: &ToolCallPart, view: &PlannerView) -> Result<PlannedCall, ReplyFault> {
        let tool = self
            .catalogue
            .named(call.name.as_str())
            .ok_or_else(|| ReplyFault::NoSuchTool(plain_name(call.name.as_str())))?;
        let json: Json =
            serde_json::from_str(call.args.as_str()).map_err(|_| ReplyFault::NotJson)?;
        if let Some(fault) = outside_known(&known_values(&tool.action(), view), &json) {
            return Err(fault);
        }
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
            effect: Some(tool.decl.effect),
        })
    }
}

impl<P: Transport> Planner for PlannerModel<P> {
    fn catalogue(&self) -> &Catalogue {
        &self.catalogue
    }

    fn set_catalogue(&mut self, catalogue: Catalogue) {
        self.catalogue = catalogue;
    }

    fn set_choices(&mut self, choices: Vec<ParamChoices>) {
        self.choices = choices;
    }

    fn voice(&self) -> Voice {
        Voice {
            role: self.role.clone(),
            options: self.options.clone(),
        }
    }

    fn set_voice(&mut self, voice: Voice) {
        self.role = voice.role;
        self.options = voice.options;
    }

    fn set_role(&mut self, role: RoleText) {
        self.role = Some(role);
    }

    async fn converse(&self, view: &PlannerView) -> Result<PlannerReply, PlanFault> {
        let (reply, route) = self.chat_routed(self.request(view)).await?;
        let mut said = self.read(reply, view)?;
        said.route = route;
        Ok(said)
    }

    async fn chat(&self, request: ChatRequest) -> Result<ChatReply, PlanFault> {
        self.chat_routed(request).await.map(|(reply, _)| reply)
    }

    async fn summarise(&self, input: &SummaryInput) -> Result<Summarised, SummariseFault> {
        self.summary_of(input).await
    }
}

/// The session type of a transport.
type AnyOrP<P> = <P as Transport>::Session;

/// What a reply that is not a chat was, for the log: a refusal or a failure with porter's own
/// words for it (never the prompt), any other kind by name.
fn not_chat(reply: &InferReply) -> String {
    match reply {
        InferReply::Refused(why) => format!("refused: {why:?}"),
        InferReply::Failed(error) => format!("failed: {error}"),
        InferReply::Cancelled => "cancelled".to_owned(),
        _ => "another kind of reply".to_owned(),
    }
}

/// A model that could not be used: the daemon's log says at which stage and why (the porter
/// error, never the prompt), the loop sees [`PlanFault::Unavailable`].
fn unavailable(stage: &str, error: &dyn std::fmt::Display) -> PlanFault {
    tracing::warn!(stage, %error, "the planner's model could not be used");
    PlanFault::Unavailable
}
