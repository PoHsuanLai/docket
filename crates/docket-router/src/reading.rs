//! What a session reads: the reader's answers, handles for the screen, memory's recall, the
//! context of the window the person prompted from. Untrusted text reaches a planner only as a
//! handle; plain text goes to the reader and to the screen.

use crate::handles::{HandleValue, context_view};
use crate::labels::{absorb, fold_labels};
use crate::router::Router;
use crate::seams::{AppLink, EventSink, LinkFault, MemoryLink, Seams};
use almanac_core::{MemoryReply, MemoryRequest};
use docket_core::{
    CallRefusal, ContextScope, ContextSnapshot, Handle, IntentsReply, NoteAsk, ReadAsk, Reader,
    RecallAsk, RecallView, RecalledLine, RecentLine, Reveal, Selection, Value, WireRefusal,
    conforms,
};
use porter_core::AppName;
use prov::{Integrity, Label, Labelled, SessionId, Source};

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

    /// `.Session.Note`: an episode the idle pass hands over, recorded as it is.
    pub(crate) fn session_note(&self, id: &SessionId, note: NoteAsk) -> IntentsReply {
        if !self.locked().sessions.contains_key(id) {
            return refuse(WireRefusal::NoSuchSession);
        }
        self.seams
            .sink()
            .append(docket_core::AuditRecord::Episode(Box::new(note.episode)));
        IntentsReply::Done
    }

    /// `.Session.Recall`: memory's answer for this session's Space, every untrusted text a
    /// handle.
    pub(crate) async fn session_recall(&self, id: &SessionId, ask: RecallAsk) -> IntentsReply {
        let Some(space) = self.locked().sessions.get(id).map(|r| r.space.clone()) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let request = match ask {
            RecallAsk::Recent(query) => MemoryRequest::Recent(space, query),
            RecallAsk::Inject(mut query) => {
                query.space = space;
                MemoryRequest::Inject(query)
            }
        };
        let Ok(reply) = self.seams.memory().ask(request).await else {
            return refuse(WireRefusal::Malformed);
        };
        let mut st = self.locked();
        let Some(record) = st.sessions.get_mut(id) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        let mut reveal = |text: &str, label: &Label| {
            let shown = record.handles.reveal(
                Labelled {
                    value: text.to_owned(),
                    label: label.clone(),
                },
                source_of(label),
            );
            if matches!(shown, Reveal::Plain(_)) && label.integrity == Integrity::Trusted {
                absorb_private(record, label);
            }
            shown
        };
        match reply {
            MemoryReply::Recent(entries) => IntentsReply::Recalled(RecallView::Recent(
                entries
                    .into_iter()
                    .map(|e| RecentLine {
                        text: e.text.as_ref().map(|t| reveal(t.as_str(), &e.label)),
                        summary: e.summary,
                        effect: e.effect,
                    })
                    .collect(),
            )),
            MemoryReply::Hits(hits) => IntentsReply::Recalled(RecallView::Hits(
                hits.into_iter()
                    .map(|h| RecalledLine {
                        text: reveal(h.text.as_str(), &h.label),
                        doc: h.doc,
                        at: h.at,
                        why: h.why,
                    })
                    .collect(),
            )),
            _ => refuse(WireRefusal::Malformed),
        }
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

fn absorb_private(record: &mut crate::state::SessionRecord, label: &Label) {
    if !matches!(label.confidentiality, prov::Confidentiality::Public) {
        absorb(record, label);
    }
}
