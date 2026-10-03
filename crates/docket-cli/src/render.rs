//! What a person reads on a terminal: short lines, no tables to parse. A program reads the JSON.

use crate::outcome::{entity_word, mentioned};
use crate::resolve::{Apps, short_name, slug, visible};
use docket_core::{
    ActionDecl, ContextView, Decimal, EntityLine, Follow, HereView, Hit, Manifest, Outcome,
    ParamDecl, ParamNeed, Preview, Reveal, SelectionView, UndoId, Value,
};
use prov::Labelled;

fn words(value: &Value) -> String {
    match value {
        Value::Text(t) | Value::Url(t) => t.clone(),
        Value::Integer(n) => n.to_string(),
        Value::Decimal(Decimal { units, scale }) => {
            let digits = usize::from(scale.0);
            let sign = if *units < 0 { "-" } else { "" };
            let body = format!("{:0>width$}", units.unsigned_abs(), width = digits + 1);
            let (whole, frac) = body.split_at(body.len() - digits);
            if digits == 0 {
                format!("{sign}{whole}")
            } else {
                format!("{sign}{whole}.{frac}")
            }
        }
        Value::Date(d) => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
        Value::DateTime(t) => format!("@{}", t.0),
        Value::Duration(s) => format!("{}s", s.0),
        Value::Choice(c) => c.to_string(),
        Value::Entity(e) => entity_word(e),
        Value::Entities(es) => es.iter().map(entity_word).collect::<Vec<_>>().join("\n"),
        Value::File(f) => f.to_string(),
        Value::Handle(h) => format!("#{}", h.0),
        Value::List(items) => items.iter().map(words).collect::<Vec<_>>().join("\n"),
        Value::Record(fields) => fields
            .iter()
            .map(|(k, v)| format!("{k}: {}", words(v)))
            .collect::<Vec<_>>()
            .join("\n"),
    }
}

fn shown(l: &Labelled<String>) -> &str {
    &l.value
}

/// A preview as lines.
pub fn preview(p: &Preview) -> String {
    match p {
        Preview::None => "(the app has nothing to show for this)".to_owned(),
        Preview::Text { heading, body } => format!("{}\n{}", shown(heading), body.value.0),
        Preview::Facts(lines) => lines
            .iter()
            .map(|f| format!("{}: {}", f.label, shown(&f.value)))
            .collect::<Vec<_>>()
            .join("\n"),
        Preview::Person { name, lines } => {
            let mut out = vec![shown(name).to_owned()];
            out.extend(
                lines
                    .iter()
                    .map(|f| format!("{}: {}", f.label, shown(&f.value))),
            );
            out.join("\n")
        }
        Preview::Thread { subject, messages } => {
            let mut out = vec![shown(subject).to_owned()];
            out.extend(
                messages
                    .iter()
                    .map(|m| format!("  {}: {}", shown(&m.from), shown(&m.snippet))),
            );
            out.join("\n")
        }
        Preview::Message { to, subject, body } => format!(
            "to: {}\nsubject: {}\n\n{}",
            to.iter().map(shown).collect::<Vec<_>>().join(", "),
            shown(subject),
            shown(body)
        ),
        Preview::TextChange { before, after } => {
            format!("- {}\n+ {}", shown(before), shown(after))
        }
        Preview::Moves(moves) => moves
            .iter()
            .map(|m| format!("{} -> {}", m.from, m.to))
            .collect::<Vec<_>>()
            .join("\n"),
        Preview::List(things) => things
            .iter()
            .map(|t| format!("{}  {}", entity_word(&t.id), shown(&t.title)))
            .collect::<Vec<_>>()
            .join("\n"),
        Preview::File(f) => format!("{} ({}, {} bytes)", shown(&f.name), f.media_type, f.size.0),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

/// A finished call: what the app said, what it returned, how to take it back.
pub fn outcome(outcome: &Outcome, undo: Option<UndoId>) -> String {
    let mut out = Vec::new();
    if let Some(said) = &outcome.said {
        out.push(said.to_string());
    }
    if let Some(value) = &outcome.value {
        out.push(words(&value.value));
    }
    if !matches!(outcome.show, Preview::None) {
        out.push(preview(&outcome.show));
    }
    if let Follow::Open(e) = &outcome.follow {
        out.push(format!("open: {}", entity_word(e)));
    }
    let (_, held) = mentioned(outcome);
    if !held.is_empty() {
        let list: Vec<String> = held.iter().map(|h| format!("#{}", h.0)).collect();
        out.push(format!("held text, for later commands: {}", list.join(" ")));
    }
    if let Some(id) = undo {
        out.push(format!("undo: quire-do undo {}", id.0));
    }
    out.join("\n")
}

/// The installed apps.
pub fn apps(apps: &Apps) -> String {
    apps.all()
        .iter()
        .map(|m| {
            let m = m.manifest();
            let n = visible(m).count();
            format!("{:<12} {}  ({n} actions)", slug(&m.app), m.app)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn params(action: &ActionDecl) -> String {
    action
        .params
        .iter()
        .map(|p: &ParamDecl| {
            let flag = format!("--{}", p.name.as_str().replace('_', "-"));
            match p.need {
                ParamNeed::Required => flag,
                ParamNeed::Optional | ParamNeed::Defaulted(_) => format!("[{flag}]"),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// One app's actions: name, effect, parameters.
pub fn list(app: &Manifest) -> String {
    visible(app)
        .map(|a| {
            let effect = serde_json::to_value(a.effect)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default();
            format!("{:<28} {:<15} {}", short_name(app, a), effect, params(a))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn line(l: &EntityLine) -> String {
    let title = match &l.title {
        Reveal::Plain(t) => t.clone(),
        Reveal::Handle(h) => format!("#{}", h.0),
    };
    format!("{}  {title}", entity_word(&l.id))
}

/// Search hits, one per line.
pub fn hits(hits: &[Hit]) -> String {
    hits.iter()
        .map(|h| format!("{}  {}", entity_word(&h.entity.id), shown(&h.entity.title)))
        .collect::<Vec<_>>()
        .join("\n")
}

/// What the app shows now.
pub fn context(view: &ContextView) -> String {
    let mut out = vec![format!("app: {}", slug(&view.app))];
    out.push(match &view.window {
        Reveal::Plain(t) => format!("window: {t}"),
        Reveal::Handle(h) => format!("window: #{}", h.0),
    });
    match &view.here {
        HereView::Nowhere => {}
        HereView::Entity(l) => out.push(format!("here: {}", line(l))),
        HereView::View { view, .. } => out.push(format!("view: {view}")),
    }
    match &view.selection {
        SelectionView::Entities { items, .. } => {
            out.extend(items.iter().map(|l| format!("selected: {}", line(l))));
        }
        SelectionView::Files(files) => {
            out.extend(files.iter().map(|f| format!("selected: {f}")));
        }
        SelectionView::Text(_) | SelectionView::Nothing => {}
    }
    out.extend(
        view.visible
            .items
            .iter()
            .map(|l| format!("visible: {}", line(l))),
    );
    out.join("\n")
}
