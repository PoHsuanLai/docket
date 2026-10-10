//! What a session reads: the reader's answers, handles for the screen, memory's recall, the
//! context of the window the person prompted from. Untrusted text reaches a planner only as a
//! handle; plain text goes to the reader and to the screen.

use crate::handles::{HandleTable, HandleValue, context_view};
use crate::labels::{absorb, fold_labels};
use crate::router::Router;
use crate::seams::{AppLink, LinkFault, Seams};
use crate::session::SessionState;
use docket_core::{
    CallRefusal, ContextScope, ContextSnapshot, Displayed, Handle, IntentsReply, ReadAsk,
    ReadFault, Reader, Resolved, Reveal, Selection, Value, WireRefusal, conforms,
};
use porter_core::AppName;
use prov::{Integrity, Label, Labelled, Quarantined, SessionId, Source};

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

/// An input of a read: its quarantined text and label. A handle of a thing or a file is held
/// but not text (`NotText`); one the session never minted is `NotHeld`.
fn text_input(
    handles: &HandleTable,
    handle: Handle,
) -> Result<(Quarantined<String>, Label), ReadFault> {
    let card = handles.card(handle).ok_or(ReadFault::NotHeld)?;
    handles
        .resolve_text(handle)
        .zip(handles.label(handle).cloned())
        .ok_or(ReadFault::NotText {
            handle,
            shape: card.shape,
        })
}

impl<S: Seams> Router<S> {
    /// `.Session.Resolve`: a handle's text, for the reader. The session now counts as having
    /// shown it to a reader.
    pub(crate) async fn session_resolve(&self, id: &SessionId, handle: Handle) -> IntentsReply {
        let held = self
            .locked()
            .sessions
            .get(id)
            .and_then(|r| r.handles.label(handle).cloned());
        if let Some(label) = held
            && label.integrity == Integrity::Untrusted
            && let Err(why) = self.ahead_of_reveal(id, None).await
        {
            return refuse(WireRefusal::Call(why));
        }
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
        IntentsReply::Resolved(Resolved { text, label })
    }

    /// `.Session.Display`: a handle's text for the screen, never for a model. A closed session
    /// shows nothing: the handles of an answer live until the answer is dismissed.
    pub(crate) fn session_display(&self, id: &SessionId, handle: Handle) -> IntentsReply {
        match self.displayed(id, handle) {
            Ok(shown) => IntentsReply::Text(shown.text),
            Err(why) => refuse(why),
        }
    }

    /// `.Session.DisplayLabelled`: the same text, with the label the session holds for it, so
    /// the screen keeps the trust mark of what it shows. The same rules as `Session.Display`.
    pub(crate) fn session_display_labelled(&self, id: &SessionId, handle: Handle) -> IntentsReply {
        match self.displayed(id, handle) {
            Ok(shown) => IntentsReply::Displayed(shown),
            Err(why) => refuse(why),
        }
    }

    fn displayed(&self, id: &SessionId, handle: Handle) -> Result<Displayed, WireRefusal> {
        let st = self.locked();
        match st.sessions.get(id) {
            None => Err(WireRefusal::NoSuchSession),
            Some(record) if matches!(record.state, SessionState::Closed(_)) => {
                Err(WireRefusal::NoSuchSession)
            }
            Some(record) => match (record.handles.display(handle), record.handles.label(handle)) {
                (Some(text), Some(label)) => Ok(Displayed {
                    text: text.to_owned(),
                    label: label.clone(),
                }),
                _ => Err(WireRefusal::Malformed),
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
            let held: Result<Vec<_>, ReadFault> = ask
                .ask
                .inputs
                .iter()
                .map(|h| text_input(&record.handles, *h))
                .collect();
            let held = match held {
                Ok(held) => held,
                Err(fault) => return refuse(WireRefusal::Read(fault)),
            };
            held.into_iter().unzip::<_, _, Vec<_>, Vec<_>>()
        };
        if labels.iter().any(|l| l.integrity == Integrity::Untrusted)
            && let Err(why) = self.ahead_of_reveal(id, None).await
        {
            return refuse(WireRefusal::Call(why));
        }
        {
            let mut st = self.locked();
            let Some(record) = st.sessions.get_mut(id) else {
                return refuse(WireRefusal::NoSuchSession);
            };
            labels.iter().for_each(|l| absorb(record, l));
        }
        let value = match self
            .seams
            .reader()
            .extract(id, ask.ask.clone(), inputs)
            .await
        {
            Ok(value) => value,
            Err(error) => return refuse(WireRefusal::Read(error.into())),
        };
        if let Err(fault) = conforms(&value, &ask.ask.want) {
            return refuse(WireRefusal::Read(ReadFault::OutOfSchema(fault)));
        }
        let mut words = Vec::new();
        texts(&value, &mut words);
        if words.is_empty() {
            return IntentsReply::Read(Reveal::Plain(value));
        }
        let label = fold_labels(labels).unwrap_or_else(crate::labels::model_label);
        if label.integrity == Integrity::Untrusted
            && let Err(why) = self.ahead_of_reveal(id, None).await
        {
            return refuse(WireRefusal::Call(why));
        }
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
        // The view holds every untrusted text of the window as a handle: the session's taint is
        // on the record before any is held.
        let mut probe = HandleTable::new();
        context_view(&snapshot, &mut probe);
        if probe.untrusted_since(0)
            && let Err(why) = self.ahead_of_reveal(id, None).await
        {
            return refuse(WireRefusal::Call(why));
        }
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
