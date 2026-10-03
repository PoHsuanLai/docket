//! What a session reads: the reader's answers, handles for the screen, memory's recall, the
//! context of the window the person prompted from. Untrusted text reaches a planner only as a
//! handle; plain text goes to the reader and to the screen.

use crate::handles::{HandleValue, context_view};
use crate::labels::{absorb, fold_labels};
use crate::router::Router;
use crate::seams::{AppLink, LinkFault, Seams};
use docket_core::{
    CallRefusal, ContextScope, ContextSnapshot, Handle, IntentsReply, ReadAsk, Reader, Reveal,
    Selection, Value, WireRefusal, conforms,
};
use porter_core::AppName;
use prov::{Label, Labelled, SessionId, Source};

fn refuse(why: WireRefusal) -> IntentsReply {
    IntentsReply::Refused(why)
}

/// How a call to an app that could not answer reads to its caller.
pub(crate) fn link_refusal(fault: LinkFault, app: &AppName) -> CallRefusal {
    match fault {
        LinkFault::Timeout => CallRefusal::Timeout,
        LinkFault::Unavailable | LinkFault::Malformed => CallRefusal::AppUnavailable(app.clone()),
    }
}

fn source_of(label: &Label) -> Source {
    label.sources.iter().next().cloned().unwrap_or(Source::User)
}

fn texts(value: &Value, into: &mut Vec<String>) {
    match value {
        Value::Text(t) | Value::Url(t) => into.push(t.clone()),
        Value::List(items) => items.iter().for_each(|v| texts(v, into)),
        Value::Record(fields) => fields.values().for_each(|v| texts(v, into)),
        _ => {}
    }
}

impl<S: Seams> Router<S> {
    /// `.Session.Resolve`: a handle's text, for the reader. The session now counts as having
    /// shown it to a reader.
    pub(crate) fn session_resolve(&self, id: &SessionId, handle: Handle) -> IntentsReply {
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let (Some(text), Some(label)) = (
            record.handles.display(handle).map(str::to_owned),
            record.handles.label(handle).cloned(),
        ) else {
            return refuse(WireRefusal::Malformed);
        };
        absorb(record, &label);
        IntentsReply::Text(text)
    }

    /// `.Session.Display`: a handle's text for the screen, never for a model.
    pub(crate) fn session_display(&self, id: &SessionId, handle: Handle) -> IntentsReply {
        let st = self.locked();
        match st.sessions.get(id) {
            None => refuse(WireRefusal::NoSuchSession),
            Some(record) => match record.handles.display(handle) {
                Some(text) => IntentsReply::Text(text.to_owned()),
                None => refuse(WireRefusal::Malformed),
            },
        }
    }

    /// `.Session.Read`: the quarantined reader answers under the planner's schema. A closed-set
    /// answer is plain; any text in it becomes a handle labelled with the join of the inputs.
    pub(crate) async fn session_read(&self, id: &SessionId, ask: ReadAsk) -> IntentsReply {
        let (inputs, labels) = {
            let mut st = self.locked();
            let Some(record) = st.sessions.get_mut(id) else {
                return refuse(WireRefusal::NoSuchSession);
            };
            let held: Option<Vec<_>> = ask
                .ask
                .inputs
                .iter()
                .map(|h| {
                    record
                        .handles
                        .resolve_text(*h)
                        .zip(record.handles.label(*h).cloned())
                })
                .collect();
            let Some(held) = held else {
                return refuse(WireRefusal::Malformed);
            };
            held.iter().for_each(|(_, l)| absorb(record, l));
            held.into_iter().unzip::<_, _, Vec<_>, Vec<_>>()
        };
        let Ok(value) = self
            .seams
            .reader()
            .extract(id, ask.ask.clone(), inputs)
            .await
        else {
            return refuse(WireRefusal::Malformed);
        };
        if conforms(&value, &ask.ask.want).is_err() {
            return refuse(WireRefusal::Malformed);
        }
        let mut words = Vec::new();
        texts(&value, &mut words);
        if words.is_empty() {
            return IntentsReply::Read(Reveal::Plain(value));
        }
        let label = fold_labels(labels).unwrap_or_else(crate::labels::model_label);
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let handle = record.handles.mint(
            Labelled {
                value: HandleValue::Text(words.join("\n")),
                label: label.clone(),
            },
            source_of(&label),
        );
        IntentsReply::Read(Reveal::Handle(handle))
    }

    /// `.Context.Current`: where the person is, in the window of `app`, the app they summoned
    /// the companion from (a launcher turn names none, so the caller says which).
    pub(crate) async fn session_context(&self, id: &SessionId, app: AppName) -> IntentsReply {
        if !self.locked().sessions.contains_key(id) {
            return refuse(WireRefusal::NoSuchSession);
        }
        let snapshot = match self
            .seams
            .link()
            .context(&app, ContextScope::ActiveWindow)
            .await
        {
            Ok(snapshot) => snapshot,
            Err(fault) => return refuse(WireRefusal::Call(link_refusal(fault, &app))),
        };
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        record.known.extend(entities_of(&snapshot));
        IntentsReply::Context(Box::new(context_view(&snapshot, &mut record.handles)))
    }
}

/// The things a snapshot names: the router showed them to the session.
fn entities_of(ctx: &ContextSnapshot) -> Vec<prov::EntityId> {
    let here = match &ctx.here {
        docket_core::Here::Entity(e) => vec![e.id.clone()],
        docket_core::Here::Nowhere | docket_core::Here::View { .. } => vec![],
    };
    let selected = match &ctx.selection {
        Selection::Entities { items, .. } => items.iter().map(|e| e.id.clone()).collect(),
        Selection::Nothing | Selection::Text(_) | Selection::Files(_) => vec![],
    };
    here.into_iter()
        .chain(selected)
        .chain(ctx.visible.items.iter().map(|e| e.id.clone()))
        .collect()
}
