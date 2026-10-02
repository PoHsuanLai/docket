//! The stripped request a reviewer sees, and how it becomes a prompt and a verdict again.
//! Untrusted arguments appear as their sources and their size, never their text.

use crate::verdict::ReviewVerdict;
use docket_core::{
    ActionRef, ArgLabels, ArgSink, CallId, CharCount, LabelText, ParamName, ReasonCode,
    ReviewError, Stage, Strictness, TaskPolicy, UserTurn, Value,
};
use model_provider::{ChoiceText, Field, FieldName, Shape};
use porter_core::{AppName, Count};
use prov::{ActionName, Effect, EntityKind, Source, SpaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How a past call ended, in a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallEndKind {
    /// It ran.
    Done,
    /// Refused.
    Denied,
    /// Not confirmed.
    Unconfirmed,
    /// Failed.
    Failed,
}

/// A past call, typed: no outcome text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TypedStep {
    /// The call, for the audit to match.
    pub call: CallId,
    /// The action.
    pub action: ActionRef,
    /// Its effect.
    pub effect: Effect,
    /// How it ended.
    pub end: CallEndKind,
    /// A reviewer's earlier code, if one ruled.
    pub verdict: Option<ReasonCode>,
}

/// An argument as the reviewer sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ArgView {
    /// Typed by the person or chosen from a trusted store.
    Trusted(Value),
    /// Somebody else's words: where from and how long, never what.
    Untrusted {
        /// The sources.
        from: BTreeSet<Source>,
        /// The size.
        size: CharCount,
    },
}

/// The call under review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProposedAction {
    /// The app.
    pub app: AppName,
    /// The action.
    pub action: ActionName,
    /// The manifest's label.
    pub label: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// The kinds it touches.
    pub kinds: BTreeSet<EntityKind>,
    /// How many things.
    pub count: Count,
    /// Each argument with the sink it feeds.
    pub args: Vec<(ParamName, ArgSink, ArgView)>,
    /// Whether it writes lasting memory.
    pub lasting: docket_core::Lasting,
}

/// What a reviewer sees: the person's turns, the typed action, labels, the task policy and the
/// typed history. Nothing a model wrote and nothing a third party wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewRequest {
    /// The Space.
    pub space: SpaceId,
    /// Its strictness.
    pub strictness: Strictness,
    /// The person's words.
    pub turns: Vec<UserTurn>,
    /// The call.
    pub proposed: ProposedAction,
    /// What the router knows about the arguments.
    pub labels: ArgLabels,
    /// What the task may do.
    pub task_policy: Option<TaskPolicy>,
    /// The session's typed history.
    pub history: Vec<TypedStep>,
}

/// The prompt a reviewer model gets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReviewPrompt {
    /// The fixed instruction for the stage.
    pub system: String,
    /// The request, rendered.
    pub user: String,
}

/// Renders a request for a stage. Deterministic; the instruction is fixed text and the request
/// holds only stripped data.
pub fn render(request: &ReviewRequest, stage: Stage) -> ReviewPrompt {
    let _ = (request, stage);
    todo!(
        "render: fixed system text per stage, then the person's turns, the typed action, the labels, the policy and the history"
    )
}

/// Reads a model's raw reply into a verdict. Unknown codes, long reasons and anything outside
/// the stage's shape are `OutOfVocabulary` or `Unparseable`; a failure is never an allow.
pub fn parse_verdict(raw: &str, stage: Stage) -> Result<ReviewVerdict, ReviewError> {
    let _ = (raw, stage);
    todo!(
        "parse_verdict: Quick reads one token (pass or flag); the others read the record of verdict, code and reason"
    )
}

const CODES: [ReasonCode; 11] = [
    ReasonCode::WithinRequest,
    ReasonCode::CoveredByTaskPolicy,
    ReasonCode::Routine,
    ReasonCode::OutsideRequest,
    ReasonCode::ScopeCreep,
    ReasonCode::Exfiltration,
    ReasonCode::Irreversible,
    ReasonCode::InjectionSuspected,
    ReasonCode::Uncertain,
    ReasonCode::ReviewerFailed,
    ReasonCode::Disagreement,
];

fn slug<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(s)) => s,
        // A unit enum serialises as a string; anything else has no slug.
        Ok(_) | Err(_) => String::new(),
    }
}

/// The shape of a stage's reply, in stoker's vocabulary, so inferd renders it for the engine
/// (a one-token choice for Quick, a record for the others) and owns validation and retry.
pub fn verdict_shape(stage: Stage) -> Shape {
    let choice =
        |words: &[&str]| Shape::Choice(words.iter().map(|w| ChoiceText((*w).to_owned())).collect());
    match stage {
        Stage::Quick => choice(&["pass", "flag"]),
        Stage::Deliberate | Stage::SecondOpinion => {
            let name = |n: &str| FieldName::new(n);
            let field = |n: &str, shape: Shape| name(n).ok().map(|name| Field { name, shape });
            let codes: Vec<String> = CODES.iter().map(slug).collect();
            let code_refs: Vec<&str> = codes.iter().map(String::as_str).collect();
            Shape::Record(
                [
                    field("verdict", choice(&["allow", "ask", "deny"])),
                    field("code", choice(&code_refs)),
                    field(
                        "reason",
                        Shape::Text {
                            max: model_provider::CharCount(200),
                        },
                    ),
                ]
                .into_iter()
                .flatten()
                .collect(),
            )
        }
    }
}
