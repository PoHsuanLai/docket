//! What the sheet shows for a call that needs the person: only the manifest's words, the
//! router's own formatting of typed arguments, and the app's dry-run preview. Agent prose is
//! never drawn, and somebody else's words are drawn quoted with where they came from.

use crate::prepared::Prepared;
use crate::state::SessionRecord;
use docket_core::{AgentReach, ArgSink, CallerRole};
use docket_core::{
    AlwaysOffer, Anchor, ArgLine, AskReason, ConfirmDetail, ConfirmId, ConfirmOffer,
    ConfirmRequest, Gesture, Limited, NeverOffer, Preview, SheetLimits, Shown, TaintNote,
};
use docket_vocab::{Seconds, Value};
use porter_core::Count;
use prov::{Actor, Effect, Integrity, Label, Source};
use std::collections::BTreeSet;

/// Whether the sheet may offer the standing grant for `p`: it comes from the terminal, the only
/// thing that asks is the terminal rule itself (no Rule of Two, untrusted sink, mass or
/// memory rule would ask on top, which a grant could not lift), and the action is neither
/// destructive nor one that asks every time.
pub(crate) fn offers_grant(p: &Prepared, why: &[AskReason]) -> bool {
    p.who.actor == Actor::Cli
        && p.decl.effect != Effect::Destructive
        && p.decl.reach == AgentReach::Offered
        && why == [AskReason::FromTerminal]
}

/// A value as words, for the sheet.
fn words(value: &Value) -> String {
    match value {
        Value::Text(t) | Value::Url(t) => t.clone(),
        Value::Integer(n) => n.to_string(),
        Value::Decimal(d) => format!("{}e-{}", d.units, d.scale.0),
        Value::Date(d) => format!("{:04}-{:02}-{:02}", d.year, d.month, d.day),
        Value::DateTime(t) => t.0.to_string(),
        Value::Duration(s) => format!("{} s", s.0),
        Value::Choice(c) => c.to_string(),
        Value::Entity(e) => e.key.as_str().to_owned(),
        Value::Entities(es) => format!("{} things", es.len()),
        Value::File(f) => f.as_str().to_owned(),
        Value::Handle(_) => "(held text)".to_owned(),
        Value::List(items) => format!("{} items", items.len()),
        Value::Record(fields) => format!("{} fields", fields.len()),
    }
}

fn shown(text: String, label: &Label) -> Shown {
    match label.integrity {
        Integrity::Trusted => Shown::Plain(text),
        Integrity::Untrusted => Shown::Quoted {
            text,
            from: label
                .sources
                .iter()
                .next()
                .cloned()
                .unwrap_or(Source::Model(prov::ModelRole::Planner)),
        },
    }
}

fn detail(p: &Prepared, preview: Option<&Preview>) -> ConfirmDetail {
    match preview {
        Some(Preview::Message { to, .. }) => ConfirmDetail::Recipients(
            to.iter()
                .map(|t| shown(t.value.clone(), &t.label))
                .collect(),
        ),
        Some(Preview::None) | None => {
            let recipients: Vec<Shown> = p
                .decl
                .params
                .iter()
                .filter(|d| d.sink == ArgSink::Recipient)
                .filter_map(|d| p.request.args.get(&d.name))
                .map(|a| shown(words(&a.value), &a.label))
                .collect();
            if recipients.is_empty() {
                ConfirmDetail::Plain
            } else {
                ConfirmDetail::Recipients(recipients)
            }
        }
        Some(other) => ConfirmDetail::Preview(other.clone()),
    }
}

fn taint_note(record: Option<&SessionRecord>) -> TaintNote {
    let sources: BTreeSet<Source> = record
        .map(|r| {
            r.seen
                .iter()
                .filter(|l| l.integrity == Integrity::Untrusted)
                .flat_map(|l| l.sources.iter().cloned())
                .collect()
        })
        .unwrap_or_default();
    if sources.is_empty() {
        TaintNote::Clean
    } else {
        TaintNote::ReadUntrusted(sources)
    }
}

/// The request the sheet draws for `p`.
pub(crate) fn confirm_request(
    id: ConfirmId,
    p: &Prepared,
    record: Option<&SessionRecord>,
    why: &[AskReason],
    preview: Option<&Preview>,
    expires: Seconds,
) -> ConfirmRequest {
    let standing = p.standing.as_ref();
    let always = standing.map_or_else(AlwaysOffer::default, |s| s.offer(&p.decl, why));
    let taint = taint_note(record);
    let destructive = p.decl.effect == Effect::Destructive;
    // A caller that holds standing grants is offered the scoped one (`always`), never the broad
    // class grant.
    // "Never" is on every sheet for an agent's call. A call that asks because the person said
    // always-ask carries that entry's reason, and "allow always" is not offered over it.
    let limits = SheetLimits {
        never: standing.map_or(NeverOffer::Withheld, |_| NeverOffer::Offered),
        held: standing.and_then(|s| match &s.limited {
            Limited::Ask(note) => Some(note.clone()),
            Limited::Free | Limited::Never(_) => None,
        }),
    };
    let offer = if crate::brief_sheet::is_brief_confirm(&p.request.action) {
        // A brief is checked every time and a "yes" for one is no grant for the next.
        ConfirmOffer::OnceOnly
    } else if limits.held.is_some() || standing.is_some_and(|s| s.holds()) {
        ConfirmOffer::OnceOnly
    } else if offers_grant(p, why) {
        ConfirmOffer::OnceOrFromTerminal
    } else if destructive || taint != TaintNote::Clean || p.who.actor == prov::Actor::Cli {
        ConfirmOffer::OnceOnly
    } else {
        ConfirmOffer::OnceOrAlways
    };
    ConfirmRequest {
        id,
        space: p.space.clone(),
        actor: p.who.actor.clone(),
        app: p.request.action.app.clone(),
        action: p.decl.label.clone(),
        effect: p.decl.effect,
        count: Count(u32::try_from(p.targets.len()).unwrap_or(u32::MAX)),
        detail: detail(p, preview),
        lines: p
            .decl
            .params
            .iter()
            .filter_map(|d| {
                p.request.args.get(&d.name).map(|a| ArgLine {
                    label: d.label.clone(),
                    value: shown(words(&a.value), &a.label),
                })
            })
            .collect(),
        why: why.to_vec(),
        taint,
        offer,
        always,
        limits,
        gesture: if destructive {
            Gesture::HoldToConfirm
        } else {
            Gesture::Press
        },
        anchor: match (&p.window, p.who.role) {
            (Some(w), _) => Anchor::Window(w.clone()),
            (None, CallerRole::Launcher | CallerRole::Field) => Anchor::Launcher,
            (None, _) => Anchor::Centre,
        },
        expires,
        editor: crate::who::route_of(&p.who),
        helper: record
            .and_then(|r| r.helper.as_ref())
            .map(|seat| seat.name.clone()),
        brief: crate::brief_sheet::sheet_of(p),
    }
}
