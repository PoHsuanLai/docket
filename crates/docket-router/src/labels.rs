//! The labels the router computes itself. A model never vouches for what it passes: a value it
//! supplies is trusted only if the router can trace it to the person's own words or to
//! something the router showed it, and a handle brings back the label it was minted with.

use crate::handles::HandleValue;
use crate::session::{SessionEvent, session_step};
use crate::state::SessionRecord;
use docket_core::{
    ActionDecl, ArgFault, ArgLabels, ArgSink, Args, ParamName, Saw, SessionSaw, SinkIntegrity,
    Value,
};
use porter_core::AppName;
use prov::{ClientName, Confidentiality, Integrity, Label, Labelled, ModelRole, Source};
use std::collections::BTreeSet;

/// Who is speaking, which sets how much of the labels it supplies the router believes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Voice {
    /// The person, through the launcher or a prompt field: its labels are the surface's.
    Person,
    /// An app calling its own actions: its labels are its own.
    App,
    /// A model: the router derives every label.
    Model,
    /// An external client: everything it sends is untrusted.
    External(ClientName),
}

fn label(integrity: Integrity, source: Source) -> Label {
    Label {
        integrity,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([source]),
    }
}

/// The person's own words.
pub(crate) fn user_label() -> Label {
    label(Integrity::Trusted, Source::User)
}

/// Text a model wrote.
pub(crate) fn model_label() -> Label {
    label(Integrity::Untrusted, Source::Model(ModelRole::Planner))
}

/// What an app authored about its own things.
pub(crate) fn app_label(app: &AppName) -> Label {
    label(Integrity::Trusted, Source::App(app.clone()))
}

/// A closed-set value a model chose (a count, a choice, a date): it cannot carry words.
fn closed_label() -> Label {
    label(Integrity::Trusted, Source::Model(ModelRole::Planner))
}

fn private(c: &Confidentiality) -> bool {
    !matches!(c, Confidentiality::Public)
}

/// The labels in `labels`, equal ones once, joined: `None` for none. A single label needs no
/// join.
pub(crate) fn fold_labels(labels: impl IntoIterator<Item = Label>) -> Option<Label> {
    let mut distinct: Vec<Label> = Vec::new();
    for l in labels {
        if !distinct.contains(&l) {
            distinct.push(l);
        }
    }
    distinct.into_iter().reduce(|a, b| a.join(&b))
}

/// Labels and resolves every argument: handles become the values they hold with their labels,
/// and a plain value gets the label its speaker's voice earns.
pub(crate) fn label_args(
    session: &SessionRecord,
    voice: &Voice,
    args: Args,
) -> Result<Args, (ParamName, ArgFault)> {
    args.into_iter()
        .map(
            |(name, arg)| match relabel(session, voice, arg.value, arg.label) {
                Ok(done) => Ok((name, done)),
                Err(fault) => Err((name, fault)),
            },
        )
        .collect()
}

fn relabel(
    session: &SessionRecord,
    voice: &Voice,
    value: Value,
    given: Label,
) -> Result<Labelled<Value>, ArgFault> {
    match value {
        Value::Handle(h) => {
            let held = session.handles.value(h).ok_or(ArgFault::UnknownHandle)?;
            let value = match &held.value {
                HandleValue::Text(t) => Value::Text(t.clone()),
                HandleValue::Entity(e) => Value::Entity(e.clone()),
                HandleValue::File(f) => Value::File(f.clone()),
            };
            Ok(Labelled {
                value,
                label: held.label.clone(),
            })
        }
        Value::List(items) => {
            let parts = items
                .into_iter()
                .map(|v| relabel(session, voice, v, given.clone()))
                .collect::<Result<Vec<_>, _>>()?;
            let (values, label) =
                split(parts, || claim(session, voice, &Value::List(vec![]), given));
            Ok(Labelled {
                value: Value::List(values),
                label,
            })
        }
        Value::Record(fields) => {
            let (names, parts): (Vec<_>, Vec<_>) = fields
                .into_iter()
                .map(|(k, v)| relabel(session, voice, v, given.clone()).map(|l| (k, l)))
                .collect::<Result<Vec<_>, _>>()?
                .into_iter()
                .unzip();
            let (values, label) = split(parts, || {
                claim(session, voice, &Value::Record(Default::default()), given)
            });
            Ok(Labelled {
                value: Value::Record(names.into_iter().zip(values).collect()),
                label,
            })
        }
        plain => {
            let label = claim(session, voice, &plain, given);
            Ok(Labelled {
                value: plain,
                label,
            })
        }
    }
}

fn split(parts: Vec<Labelled<Value>>, empty: impl FnOnce() -> Label) -> (Vec<Value>, Label) {
    let label = fold_labels(parts.iter().map(|p| p.label.clone())).unwrap_or_else(empty);
    (parts.into_iter().map(|p| p.value).collect(), label)
}

/// The label a plain value earns from its speaker.
fn claim(session: &SessionRecord, voice: &Voice, value: &Value, given: Label) -> Label {
    match voice {
        Voice::Person | Voice::App => given,
        Voice::External(client) => label(Integrity::Untrusted, Source::Mcp(client.clone())),
        Voice::Model => trace(session, value),
    }
}

/// A model's value is trusted when it is words of the person's own turns, a thing the router
/// showed the session, or a closed-set value; anything else is the model's own writing.
fn trace(session: &SessionRecord, value: &Value) -> Label {
    let spoken =
        |text: &str| !text.is_empty() && session.turns.iter().any(|t| t.text.contains(text));
    match value {
        Value::Text(t) | Value::Url(t) if spoken(t) => user_label(),
        Value::Text(_) | Value::Url(_) => model_label(),
        Value::File(f) if spoken(f.as_str()) => user_label(),
        Value::File(_) => model_label(),
        Value::Entity(e) if session.known.contains(e) => app_label(&e.app),
        Value::Entities(es) if !es.is_empty() && es.iter().all(|e| session.known.contains(e)) => {
            app_label(&es[0].app)
        }
        Value::Entity(_) | Value::Entities(_) => model_label(),
        Value::Handle(_) | Value::List(_) | Value::Record(_) => model_label(),
        Value::Integer(_)
        | Value::Decimal(_)
        | Value::Date(_)
        | Value::DateTime(_)
        | Value::Duration(_)
        | Value::Choice(_) => closed_label(),
    }
}

/// The integrity of what each sink receives: the least trusted argument that feeds it. A sink
/// no argument feeds receives nothing untrusted.
pub(crate) fn sink_integrity(decl: &ActionDecl, args: &Args) -> SinkIntegrity {
    let of = |wanted: &[ArgSink]| {
        let untrusted = decl
            .params
            .iter()
            .filter(|p| wanted.contains(&p.sink))
            .filter_map(|p| args.get(&p.name))
            .any(|a| a.label.integrity == Integrity::Untrusted);
        if untrusted {
            Integrity::Untrusted
        } else {
            Integrity::Trusted
        }
    };
    SinkIntegrity {
        recipient: of(&[ArgSink::Recipient]),
        destination: of(&[ArgSink::Destination, ArgSink::Query]),
        body: of(&[ArgSink::Body]),
        path: of(&[ArgSink::Path]),
    }
}

/// What the router knows about the arguments of one call.
pub(crate) fn arg_labels(args: &Args, session: &SessionRecord) -> ArgLabels {
    ArgLabels {
        per_arg: args
            .iter()
            .map(|(k, v)| (k.clone(), v.label.clone()))
            .collect(),
        planner: planner_integrity(&session.saw),
        saw: session.saw,
    }
}

/// A planner that has seen untrusted content is untrusted.
pub(crate) fn planner_integrity(saw: &SessionSaw) -> Integrity {
    match saw.untrusted {
        Saw::Seen => Integrity::Untrusted,
        Saw::NotSeen => Integrity::Trusted,
    }
}

/// The confidentiality of everything a call carries.
pub(crate) fn args_confidentiality(args: &Args) -> Confidentiality {
    args.values()
        .map(|a| a.label.confidentiality.clone())
        .fold(Confidentiality::Public, |a, b| a.join(&b))
}

/// Takes `label` into the session: what it now has seen can only grow, and a plain untrusted
/// reveal taints it for good.
pub(crate) fn absorb(session: &mut SessionRecord, label: &Label) {
    if label.integrity == Integrity::Untrusted {
        session.saw.untrusted = Saw::Seen;
        session.state = session_step(session.state, SessionEvent::UntrustedReveal).0;
    }
    if private(&label.confidentiality) {
        session.saw.private = Saw::Seen;
    }
    if !session.seen.contains(label) {
        session.seen.push(label.clone());
    }
}
