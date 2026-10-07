//! The policy a model's draft is, once everything in it is checked against the catalogue and the
//! turns. Nothing in the draft is believed as written: an action must be in the catalogue, the
//! ceiling never exceeds what the chosen actions need, and a recipient, destination or path is
//! kept only when the person wrote it in a turn. Pure: no model, no clock.

use docket_core::{
    ActionCard, ActionMatch, ActionRef, LabelText, TaskPolicy, TaskPolicyState, TrustedPattern,
    UserTurn, Value,
};
use porter_core::Count;
use prov::{Effect, EntityKind, SpaceId, TaskId, UnixSeconds};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeSet;

/// The most things one call of a derived policy may touch, whatever the model says.
pub(crate) const MOST_AT_ONCE: u32 = 100;

/// What the model answers.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Draft {
    pub(crate) actions: Vec<String>,
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
pub(crate) struct AppUpTo {
    app: String,
    up_to: Effect,
}

/// How a catalogue entry is named in the schema: the app, a space, the action.
pub(crate) fn key(card: &ActionCard) -> String {
    format!("{} {}", card.action.app, card.action.name)
}

fn effects() -> Vec<&'static str> {
    vec!["read", "undoable_write", "outbound", "destructive"]
}

/// The JSON Schema of the draft: the listed actions and apps only.
pub(crate) fn schema(catalogue: &[ActionCard]) -> String {
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
pub(crate) fn policy_of(
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
