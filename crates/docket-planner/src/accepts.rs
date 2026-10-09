//! Which actions of the view take a thing of a kind, said from the cards alone: the kind, the
//! parameter or target it fills, and the action's name. Nothing of the thing's content, so the
//! words are the same on every turn while the view's actions are.

use docket_core::{ActionCard, TargetKind};
use prov::EntityKind;
use serde_json::Value as Json;
use std::collections::BTreeMap;

/// The most actions named for one place a thing can go; the rest are counted.
const MOST_ACTIONS: usize = 6;

/// Where in a call a thing is named.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Place {
    /// The call's `target`.
    Target,
    /// The argument of this name.
    Param(String),
}

/// Whether a parameter's schema has a place for a thing of `kind` (an entity or a handle for it).
fn takes_kind(schema: &Json, kind: &str) -> bool {
    match schema {
        Json::Object(fields) => {
            let own = fields
                .get("properties")
                .and_then(|p| p.get("kind"))
                .and_then(|k| k.get("const"))
                .and_then(Json::as_str);
            own == Some(kind) || fields.values().any(|v| takes_kind(v, kind))
        }
        Json::Array(items) => items.iter().any(|v| takes_kind(v, kind)),
        _ => false,
    }
}

fn places(card: &ActionCard, kind: &EntityKind) -> Vec<Place> {
    let target = matches!(&card.on, TargetKind::One(k) | TargetKind::Many(k) if k == kind)
        .then_some(Place::Target);
    let params = card
        .tool
        .0
        .get("properties")
        .and_then(Json::as_object)
        .into_iter()
        .flatten()
        .filter(|(_, schema)| takes_kind(schema, kind.as_str()))
        .map(|(name, _)| Place::Param(name.clone()));
    target.into_iter().chain(params).collect()
}

fn said(place: &Place, names: &[String]) -> String {
    let shown = names.iter().take(MOST_ACTIONS).cloned().collect::<Vec<_>>();
    let more = names.len().saturating_sub(MOST_ACTIONS);
    let rest = if more > 0 {
        format!(", +{more} more")
    } else {
        String::new()
    };
    let how = match place {
        Place::Target => "as target".to_owned(),
        Place::Param(name) => format!("as \"{name}\""),
    };
    format!("{how} in {}{rest}", shown.join(", "))
}

/// Where a thing of `kind` can be used, as `as target in mail.thread.read; as "to" in
/// mail.message.forward, mail.message.send`, or nothing when no action in the view takes it.
pub(crate) fn used_as(kind: &EntityKind, actions: &[ActionCard]) -> Option<String> {
    let mut by_place: BTreeMap<Place, Vec<String>> = BTreeMap::new();
    for card in actions {
        let name = card.action.name.as_str().to_owned();
        for place in places(card, kind) {
            let names = by_place.entry(place).or_default();
            if !names.contains(&name) {
                names.push(name.clone());
            }
        }
    }
    if by_place.is_empty() {
        return None;
    }
    let parts: Vec<String> = by_place.iter().map(|(p, n)| said(p, n)).collect();
    Some(parts.join("; "))
}

#[cfg(test)]
pub(crate) mod fixtures {
    use docket_core::{
        ActionCard, ActionRef, AgentReach, LabelText, Lasting, TargetKind, ToolSchema,
    };
    use prov::{ActionName, Effect, EntityKind};
    use serde_json::json;

    fn entity(kind: &str) -> serde_json::Value {
        json!({ "anyOf": [
            { "type": "object", "properties": { "app": {}, "kind": { "const": kind }, "key": {} } },
            { "type": "object", "properties": { "handle": { "type": "integer" } } },
        ] })
    }

    /// An action of `org.quire.Mail` on `on`, with these parameters (name, schema).
    pub(crate) fn card(
        name: &str,
        on: TargetKind,
        params: &[(&str, serde_json::Value)],
    ) -> ActionCard {
        let properties: serde_json::Map<String, serde_json::Value> = params
            .iter()
            .map(|(n, s)| ((*n).to_owned(), s.clone()))
            .collect();
        ActionCard {
            action: ActionRef {
                app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
                name: ActionName::parse(name).expect("name"),
            },
            label: LabelText::parse("An action").expect("label"),
            effect: Effect::Read,
            on,
            tool: ToolSchema(json!({ "type": "object", "properties": properties })),
            reach: AgentReach::Offered,
            lasting: Lasting::No,
        }
    }

    pub(crate) fn kind(text: &str) -> EntityKind {
        EntityKind::parse(text).expect("kind")
    }

    /// The mail actions a forward needs: threads are read, contacts are sent to.
    pub(crate) fn mail() -> Vec<ActionCard> {
        let one = |k: &str| TargetKind::One(kind(k));
        vec![
            card("mail.thread.read", one("mail.thread"), &[]),
            card(
                "mail.message.forward",
                one("mail.thread"),
                &[("to", entity("mail.contact"))],
            ),
            card(
                "mail.message.send",
                TargetKind::Nothing,
                &[
                    ("to", entity("mail.contact")),
                    ("subject", json!({ "type": "string" })),
                ],
            ),
            card(
                "mail.contact.search",
                TargetKind::Nothing,
                &[("query", json!({ "type": "string" }))],
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::fixtures::{card, kind, mail};
    use super::*;

    #[test]
    fn a_kind_is_used_where_an_action_targets_it_or_takes_it_as_an_argument() {
        let cards = mail();
        assert_eq!(
            used_as(&kind("mail.contact"), &cards).as_deref(),
            Some("as \"to\" in mail.message.forward, mail.message.send")
        );
        assert_eq!(
            used_as(&kind("mail.thread"), &cards).as_deref(),
            Some("as target in mail.thread.read, mail.message.forward")
        );
    }

    #[test]
    fn a_kind_no_action_takes_is_not_said_to_be_used() {
        assert_eq!(used_as(&kind("mail.draft"), &mail()), None);
        let none = [card("mail.contact.search", TargetKind::Nothing, &[])];
        assert_eq!(used_as(&kind("mail.contact"), &none), None);
    }

    #[test]
    fn a_long_list_of_actions_is_cut_and_counted() {
        let many: Vec<ActionCard> = (0..9)
            .map(|n| {
                card(
                    &format!("mail.act{n}"),
                    TargetKind::One(kind("mail.thread")),
                    &[],
                )
            })
            .collect();
        let said = used_as(&kind("mail.thread"), &many).expect("said");
        assert!(said.ends_with("mail.act5, +3 more"), "{said}");
    }
}
