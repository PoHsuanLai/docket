//! What a successful run prints as JSON: `{vocab, outcome, undo, entities, handles}` for a call,
//! with the things and the held handles the outcome mentions listed apart so a script can pass
//! them to the next command (`<kind>:<key>` or `#<n>`).

use docket_core::{
    ContextView, EntityLine, Follow, Handle, Hit, IntentsVocab, Outcome, Preview, Reveal,
    SelectionView, UndoId, Value as Val,
};
use prov::EntityId;
use serde_json::{Value, json};

/// One thing as `{app, kind, key}`.
pub fn entity_json(id: &EntityId) -> Value {
    json!({ "app": id.app, "kind": id.kind, "key": id.key })
}

/// A thing as the words a later command takes.
pub fn entity_word(id: &EntityId) -> String {
    format!("{}:{}", id.kind, id.key)
}

fn value_things(value: &Val, things: &mut Vec<EntityId>, held: &mut Vec<Handle>) {
    match value {
        Val::Entity(e) => things.push(e.clone()),
        Val::Entities(es) => things.extend(es.iter().cloned()),
        Val::Handle(h) => held.push(*h),
        Val::List(items) => items.iter().for_each(|v| value_things(v, things, held)),
        Val::Record(fields) => fields.values().for_each(|v| value_things(v, things, held)),
        _ => {}
    }
}

/// The things and the handles an outcome names, each once, in order.
pub fn mentioned(outcome: &Outcome) -> (Vec<EntityId>, Vec<Handle>) {
    let (mut things, mut held) = (Vec::new(), Vec::new());
    if let Some(value) = &outcome.value {
        value_things(&value.value, &mut things, &mut held);
    }
    if let Follow::Open(e) = &outcome.follow {
        things.push(e.clone());
    }
    if let Preview::List(list) = &outcome.show {
        things.extend(list.iter().map(|r| r.id.clone()));
    }
    let mut seen = Vec::new();
    things.retain(|t| {
        let fresh = !seen.contains(t);
        seen.push(t.clone());
        fresh
    });
    held.dedup();
    (things, held)
}

/// A finished call.
pub fn outcome_json(outcome: &Outcome, undo: Option<UndoId>) -> Value {
    let (things, held) = mentioned(outcome);
    json!({
        "vocab": IntentsVocab::CURRENT,
        "outcome": {
            "said": outcome.said,
            "value": outcome.value.as_ref().map(|v| &v.value),
            "show": outcome.show,
            "follow": outcome.follow,
        },
        "undo": undo.map(|id| json!({ "id": id.0 })),
        "entities": things.iter().map(entity_json).collect::<Vec<_>>(),
        "handles": held.iter().map(|h| h.0).collect::<Vec<_>>(),
    })
}

/// A dry run.
pub fn dry_run_json(preview: &Preview) -> Value {
    json!({ "vocab": IntentsVocab::CURRENT, "preview": preview, "entities": [], "handles": [] })
}

/// A search.
pub fn hits_json(hits: &[Hit]) -> Value {
    json!({
        "vocab": IntentsVocab::CURRENT,
        "hits": hits.iter().map(|h| json!({
            "entity": entity_json(&h.entity.id),
            "title": h.entity.title.value,
            "subtitle": h.entity.subtitle.value,
            "why": h.why,
        })).collect::<Vec<_>>(),
        "entities": hits.iter().map(|h| entity_json(&h.entity.id)).collect::<Vec<_>>(),
        "handles": [],
    })
}

fn line_handles(line: &EntityLine, held: &mut Vec<u64>) {
    for words in [&line.title, &line.subtitle] {
        if let Reveal::Handle(h) = words {
            held.push(h.0);
        }
    }
}

/// What the app shows now: its text is held as handles where it is somebody else's.
pub fn context_json(view: &ContextView) -> Value {
    let mut held = Vec::new();
    let mut things = Vec::new();
    let mut note = |line: &EntityLine| {
        things.push(entity_json(&line.id));
        line_handles(line, &mut held);
    };
    view.visible.items.iter().for_each(&mut note);
    if let SelectionView::Entities { items, .. } = &view.selection {
        items.iter().for_each(&mut note);
    }
    if let docket_core::HereView::Entity(line) = &view.here {
        note(line);
    }
    held.sort_unstable();
    held.dedup();
    json!({
        "vocab": IntentsVocab::CURRENT,
        "context": view,
        "entities": things,
        "handles": held,
    })
}
