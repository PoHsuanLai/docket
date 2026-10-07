//! The policy writer over inferd: it reads the person's own turns and the action catalogue, never
//! content, and asks a model for a draft of the task's policy as JSON under a schema. Nothing in
//! the draft is believed as written: an action must be in the catalogue, the ceiling never
//! exceeds what the chosen actions need, and a recipient, destination or path is kept only when
//! the person wrote it in a turn. The router stamps `from` and `expires` and bounds the result
//! by the grants (`Router::bound_policy`); if the writer fails there is no policy and every
//! non-read call is outside.

use crate::infer::{Discard, chat_of, turn};
use docket_core::{
    ActionCard, ActionMatch, ActionRef, LabelText, PolicyWriter, ReviewError, TaskPolicy,
    TaskPolicyState, TrustedPattern, UserTurn, Value,
};
use docket_dbus::InferLink;
use porter_client::Transport;
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{Count, DataClass, Need, Permille, Tier, Tokens};
use porter_infer::{
    ChatControl, ChatMessage, ChatRequest, InferRequest, Knob, MessagePart, ModelError, Reasoning,
    ReplyShape, Role, Sampling, ToolChoice, ToolParallelism,
};
use prov::{Effect, EntityKind, SpaceId, TaskId, UnixSeconds};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

/// The most things one call of a derived policy may touch, whatever the model says.
const MOST_AT_ONCE: u32 = 100;

/// The words the model is given. The turns are the person's own; nothing else is in the prompt.
const INSTRUCTION: &str = "You write a task policy: the least an assistant needs to do what \
the person asked, and no more. You are given the person's own words and the list of actions \
that exist. Choose only listed actions. Choose a ceiling no higher than the chosen actions \
need. Name a recipient, destination or path only if the person wrote it. When the person \
asked only to look, choose reading actions only.";

/// The policy writer: reads the person's turns and the action catalogue, never content.
#[derive(Debug)]
pub struct InferdWriter<T: Transport> {
    transport: T,
}

impl<T: Transport> InferdWriter<T> {
    /// Asks through `transport`.
    pub fn new(transport: T) -> Self {
        Self { transport }
    }
}

impl InferdWriter<InferLink> {
    /// Asks inferd over the session bus.
    pub fn on_bus(connection: &docket_dbus::BusConnection) -> Self {
        Self::new(crate::infer::inferd_transport(connection))
    }
}

/// What the model answers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    actions: Vec<String>,
    apps: Vec<AppUpTo>,
    kinds: Vec<String>,
    ceiling: Effect,
    max_count: u32,
    recipients: Vec<String>,
    destinations: Vec<String>,
    paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct AppUpTo {
    app: String,
    up_to: Effect,
}

/// How a catalogue entry is named in the schema: the app, a space, the action.
fn key(card: &ActionCard) -> String {
    format!("{} {}", card.action.app, card.action.name)
}

fn effects() -> Vec<&'static str> {
    vec!["read", "undoable_write", "outbound", "destructive"]
}

/// The JSON Schema of the draft: the listed actions and apps only.
fn schema(catalogue: &[ActionCard]) -> String {
    let actions: Vec<String> = catalogue.iter().map(key).collect();
    let apps: BTreeSet<String> = catalogue.iter().map(|c| c.action.app.to_string()).collect();
    let strings =
        json!({ "type": "array", "items": { "type": "string", "maxLength": 200 }, "maxItems": 20 });
    json!({
        "type": "object",
        "properties": {
            "actions": { "type": "array", "items": { "type": "string", "enum": actions }, "maxItems": 64 },
            "apps": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "app": { "type": "string", "enum": apps },
                        "up_to": { "type": "string", "enum": effects() }
                    },
                    "required": ["app", "up_to"],
                    "additionalProperties": false
                },
                "maxItems": 16
            },
            "kinds": { "type": "array", "items": { "type": "string", "maxLength": 64 }, "maxItems": 16 },
            "ceiling": { "type": "string", "enum": effects() },
            "max_count": { "type": "integer", "minimum": 1, "maximum": MOST_AT_ONCE },
            "recipients": strings,
            "destinations": strings,
            "paths": strings
        },
        "required": ["actions", "apps", "kinds", "ceiling", "max_count", "recipients", "destinations", "paths"],
        "additionalProperties": false
    })
    .to_string()
}

/// The prompt: the catalogue (names, labels and effects, all the apps' own words) and the
/// person's turns, numbered.
fn prompt(turns: &[UserTurn], catalogue: &[ActionCard]) -> String {
    let actions: String = catalogue
        .iter()
        .map(|c| format!("- {} ({:?}): {}\n", key(c), c.effect, c.label))
        .collect();
    let said: String = turns
        .iter()
        .map(|t| format!("[{}] {}\n", t.id.0, t.text))
        .collect();
    format!("Actions that exist:\n{actions}\nWhat the person said:\n{said}")
}

fn request(turns: &[UserTurn], catalogue: &[ActionCard]) -> ChatRequest {
    let message = |role, text| ChatMessage {
        role,
        parts: vec![MessagePart::Text(text)],
    };
    ChatRequest {
        messages: vec![
            message(Role::System, INSTRUCTION.to_owned()),
            message(Role::User, prompt(turns, catalogue)),
        ],
        shape: ReplyShape::Json(schema(catalogue)),
        tier: Tier::Fast,
        // The person's own words: their floor is this computer.
        class: DataClass::Prompt,
        usage: Usage::Interactive,
        tools: Vec::new(),
        control: ChatControl {
            tool_choice: ToolChoice::Never,
            tool_calls: ToolParallelism::One,
            max_output: Knob::Set(Tokens(600)),
            reasoning: Reasoning::Off,
            sampling: Knob::Set(Sampling {
                temperature: Permille(0),
                top_p: Knob::Off,
                top_k: Knob::Off,
                min_p: Knob::Off,
                seed: Knob::Off,
            }),
            stop: Vec::new(),
        },
    }
}

fn need(request: &ChatRequest) -> Need {
    Need::Llm(LlmNeed {
        features: BTreeSet::from([LlmFeature::Chat, LlmFeature::StructuredOutput]),
        context: Tokens(
            u32::try_from(request.messages.iter().map(message_len).sum::<usize>() / 3)
                .unwrap_or(u32::MAX)
                .saturating_add(600),
        ),
    })
}

fn message_len(message: &ChatMessage) -> usize {
    message
        .parts
        .iter()
        .map(|p| match p {
            MessagePart::Text(t) => t.len(),
            _ => 0,
        })
        .sum()
}

/// The words of a turn that can name something: runs of the characters an address, a domain or a
/// path is made of, with a sentence's closing dot taken off, lower-cased.
fn tokens(turns: &[UserTurn]) -> Vec<String> {
    turns
        .iter()
        .flat_map(|t| {
            t.text
                .split(|c: char| {
                    !(c.is_alphanumeric() || matches!(c, '.' | '/' | '@' | '_' | '-' | '+' | '~'))
                })
                .map(|w| w.trim_end_matches('.').to_lowercase())
                .collect::<Vec<_>>()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Whether the person wrote `text` as a whole word of their own: not a piece of one. The word
/// `com` is not named by `alice@example.com`, nor `/` by any path, whatever a model draws from
/// what the person wrote.
fn named_by_person(turns: &[UserTurn], text: &str) -> bool {
    let needle = text.trim().to_lowercase();
    !needle.is_empty() && tokens(turns).contains(&needle)
}

/// Whether the person wrote an address or domain that is `text`: the address itself, or the
/// domain of an address they wrote, or the domain on its own.
fn addressed_by_person(turns: &[UserTurn], text: &str) -> bool {
    let needle = text.trim().trim_start_matches('@').to_lowercase();
    !needle.is_empty()
        && tokens(turns)
            .iter()
            .any(|w| *w == needle || w.rsplit_once('@').is_some_and(|(_, host)| host == needle))
}

/// A recipient or a destination the person named: a whole address or value, or a domain.
fn trusted_value(turns: &[UserTurn], text: &str) -> Option<TrustedPattern> {
    let text = text.trim();
    if !addressed_by_person(turns, text) {
        return None;
    }
    match text.contains('@') && !text.starts_with('@') {
        true => Some(TrustedPattern::Exact(Value::Text(text.to_owned()))),
        false => Some(TrustedPattern::Domain(
            text.trim_start_matches('@').to_owned(),
        )),
    }
}

/// A path the person wrote, whole, and not the root of everything: what a writer may draw a
/// `paths` entry from.
fn path_of_person(turns: &[UserTurn], path: &str) -> Option<docket_core::FileRef> {
    let path = path.trim();
    let whole = named_by_person(turns, path) || named_by_person(turns, path.trim_end_matches('/'));
    let root = matches!(path.trim_end_matches('/'), "" | "~");
    (whole && !root)
        .then(|| docket_core::FileRef::parse(path).ok())
        .flatten()
}

/// The policy a draft is, once everything in it is checked against the catalogue and the turns.
fn policy_of(
    draft: Draft,
    task: &TaskId,
    turns: &[UserTurn],
    catalogue: &[ActionCard],
    space: &SpaceId,
) -> TaskPolicy {
    let chosen: Vec<&ActionCard> = draft
        .actions
        .iter()
        .filter_map(|name| catalogue.iter().find(|c| &key(c) == name))
        .collect();
    let mut actions: BTreeSet<ActionMatch> = chosen
        .iter()
        .map(|c| {
            ActionMatch::One(ActionRef {
                app: c.action.app.clone(),
                name: c.action.name.clone(),
            })
        })
        .collect();
    let mut needed = chosen
        .iter()
        .map(|c| c.effect)
        .max()
        .unwrap_or(Effect::Read);
    for entry in &draft.apps {
        let known = catalogue.iter().any(|c| c.action.app.as_str() == entry.app);
        if let (true, Ok(app)) = (known, porter_core::AppName::parse(&entry.app)) {
            actions.insert(ActionMatch::AppUpTo(app, entry.up_to));
            needed = needed.max(entry.up_to);
        }
    }
    let patterns = |texts: &[String]| -> Vec<TrustedPattern> {
        texts
            .iter()
            .filter_map(|t| trusted_value(turns, t))
            .collect()
    };
    TaskPolicy {
        task: task.clone(),
        space: space.clone(),
        from: turns.iter().map(|t| t.id).collect(),
        actions,
        kinds: draft
            .kinds
            .iter()
            .filter_map(|k| EntityKind::parse(k).ok())
            .collect(),
        // Never above what the chosen actions need, whatever the model asked for.
        ceiling: draft.ceiling.min(needed),
        max_count: Count(draft.max_count.clamp(1, MOST_AT_ONCE)),
        recipients: patterns(&draft.recipients),
        destinations: patterns(&draft.destinations),
        paths: draft
            .paths
            .iter()
            .filter_map(|p| path_of_person(turns, p))
            .map(TrustedPattern::Under)
            .collect(),
        // The router lowers this to now plus `task_policy_max` (`bound_policy` takes the
        // smaller): a zero here would make every derived policy expired the moment a real clock
        // reads past the epoch (found by the first live-eval run).
        expires: UnixSeconds(i64::MAX),
        // The person's own last words, as the sheet will quote them.
        rationale: rationale_of(turns),
        state: TaskPolicyState::Active,
    }
}

/// The person's last turn, shortened, as the reason shown when the policy widens.
fn rationale_of(turns: &[UserTurn]) -> LabelText {
    let said = turns.last().map_or("", |t| t.text.as_str());
    let line: String = said
        .lines()
        .next()
        .unwrap_or_default()
        .chars()
        .filter(|c| !c.is_control())
        .take(120)
        .collect();
    LabelText::parse(line.trim())
        .or_else(|_| LabelText::parse("what you asked"))
        .expect("`what you asked` is valid label text")
}

fn failed(error: ModelError) -> ReviewError {
    match error {
        ModelError::Unparseable | ModelError::Unreadable => ReviewError::Unparseable,
        ModelError::Unreachable
        | ModelError::RateLimited(_)
        | ModelError::Unauthorized
        | ModelError::Refused
        | ModelError::NotReady
        | ModelError::ContextOverflow => ReviewError::Unavailable,
    }
}

impl<T: Transport> PolicyWriter for InferdWriter<T> {
    async fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> Result<TaskPolicy, ReviewError> {
        let request = request(turns, catalogue);
        let mut session = self
            .transport
            .open(&need(&request), request.class, request.tier)
            .await
            .map_err(|_| ReviewError::Unavailable)?;
        let reply = turn(&mut session, InferRequest::Chat(request), &mut Discard)
            .await
            .map_err(failed)?;
        let chat = chat_of(reply).map_err(failed)?;
        match chat.stop {
            porter_infer::StopReason::EndTurn | porter_infer::StopReason::StopSequence => {}
            _ => return Err(ReviewError::Unparseable),
        }
        let draft: Draft =
            serde_json::from_str(&chat.text).map_err(|_| ReviewError::Unparseable)?;
        Ok(policy_of(draft, task, turns, catalogue, space))
    }
}
