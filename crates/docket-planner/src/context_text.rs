//! Where the person is, as the planner reads it: one short line of JSON-like text. A thing leads
//! with its own handle (`"handle": 12`, the number to name it by) and then its words as handles
//! (`#13`); trusted words are quoted. What is empty is left out, and a thing's id is not shown:
//! the handle is what names it.

use docket_core::{
    ContextView, EntityLine, HereView, Reveal, SelectionView, TextTargetView, VisibleView,
};
use std::collections::BTreeSet;

fn json<T: serde::Serialize>(value: &T) -> String {
    serde_json::to_string(value).unwrap_or_default()
}

fn words(reveal: &Reveal<String>) -> String {
    match reveal {
        Reveal::Plain(text) => json(text),
        Reveal::Handle(handle) => format!("#{}", handle.0),
    }
}

/// Whether a thing says its kind (a list of one kind says it once).
#[derive(Clone, Copy)]
enum Kind {
    Said,
    Implied,
}

fn thing(line: &EntityLine, kind: Kind) -> String {
    let kind = match kind {
        Kind::Said => format!(",\"kind\":{}", json(&line.id.kind)),
        Kind::Implied => String::new(),
    };
    format!(
        "{{\"handle\":{}{kind},\"title\":{},\"subtitle\":{}}}",
        line.handle.0,
        words(&line.title),
        words(&line.subtitle)
    )
}

fn things(items: &[EntityLine], kind: Kind) -> String {
    let items: Vec<String> = items.iter().map(|l| thing(l, kind)).collect();
    format!("[{}]", items.join(","))
}

fn visible(view: &VisibleView) -> Option<String> {
    if view.items.is_empty() {
        return None;
    }
    let (kind, said) = match &view.kind {
        Some(kind) => (format!("\"kind\":{},", json(kind)), Kind::Implied),
        None => (String::new(), Kind::Said),
    };
    let total = if usize::try_from(view.total.0).is_ok_and(|n| n == view.items.len()) {
        String::new()
    } else {
        format!(",\"total\":{}", view.total.0)
    };
    Some(format!(
        "\"visible\":{{{kind}\"items\":{}{total}}}",
        things(&view.items, said)
    ))
}

/// The context as the prompt shows it.
pub(crate) fn context_text(view: &ContextView) -> String {
    let mut parts = vec![format!("\"app\":{}", json(&view.app.as_str()))];
    if view.window != Reveal::Plain(String::new()) {
        parts.push(format!("\"window\":{}", words(&view.window)));
    }
    match &view.here {
        HereView::Nowhere => {}
        HereView::Entity(line) => parts.push(format!("\"here\":{}", thing(line, Kind::Said))),
        HereView::View { view, query } => {
            let query = query
                .as_ref()
                .map(|q| format!(",\"query\":{}", words(q)))
                .unwrap_or_default();
            parts.push(format!(
                "\"here\":{{\"view\":{}{query}}}",
                json(&view.as_str())
            ));
        }
    }
    match &view.selection {
        SelectionView::Nothing => {}
        SelectionView::Entities { kind, items } => parts.push(format!(
            "\"selection\":{{\"kind\":{},\"items\":{}}}",
            json(kind),
            things(items, Kind::Implied)
        )),
        SelectionView::Text(text) => {
            parts.push(format!("\"selection\":{{\"text\":{}}}", words(text)))
        }
        SelectionView::Files(files) => {
            parts.push(format!("\"selection\":{{\"files\":{}}}", files.len()));
        }
    }
    parts.extend(visible(&view.visible));
    if let TextTargetView::Field {
        purpose, around, ..
    } = &view.text_target
    {
        parts.push(format!(
            "\"text_field\":{{\"purpose\":{},\"around\":{}}}",
            json(purpose),
            words(around)
        ));
    }
    format!("{{{}}}", parts.join(","))
}

fn note(held: &mut BTreeSet<u64>, reveal: &Reveal<String>) {
    if let Reveal::Handle(h) = reveal {
        held.insert(h.0);
    }
}

/// The handles the context shows: a thing's own and its words'.
pub(crate) fn shown_in(view: &ContextView) -> BTreeSet<u64> {
    let mut held = BTreeSet::new();
    note(&mut held, &view.window);
    let mut lines: Vec<&EntityLine> = view.visible.items.iter().collect();
    match &view.here {
        HereView::Entity(line) => lines.push(line),
        HereView::View {
            query: Some(query), ..
        } => note(&mut held, query),
        HereView::Nowhere | HereView::View { query: None, .. } => {}
    }
    match &view.selection {
        SelectionView::Entities { items, .. } => lines.extend(items),
        SelectionView::Text(text) => note(&mut held, text),
        SelectionView::Nothing | SelectionView::Files(_) => {}
    }
    if let TextTargetView::Field { around, .. } = &view.text_target {
        note(&mut held, around);
    }
    for line in lines {
        note(&mut held, &line.title);
        note(&mut held, &line.subtitle);
        held.insert(line.handle.0);
    }
    held
}
