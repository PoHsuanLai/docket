//! Sending and receiving messages. The router stamps the sender, the label and the time; a
//! message is input and carries no authority: delivery adds to the receiver's inbox and joins
//! its taint, and the request in it is evaluated by the receiver's own policy and gate when
//! the receiver acts on it. Across Spaces it shows on the roster and grants no memory read.

use crate::labels::{absorb, fold_labels, model_label, user_label};
use crate::messages::{assemble, check_stamped, inbound_line, report_status};
use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::session::SessionState;
use crate::state::{RouterState, SessionRecord};
use crate::tasks::TaskState;
use docket_core::{
    AuditRecord, CallerId, CallerRole, Delivery, DraftPart, Handle, InboxAsk, IntentsReply,
    MessageDraft, SendRefusal, WireRefusal, halted,
};
use prov::{
    Actor, Address, AgentRef, Label, Message, MessageId, MessageText, ReportStatus, SessionId,
};
use std::collections::BTreeMap;

/// How recently a session was opened: ids are minted in order.
pub(crate) fn age(id: &SessionId) -> u64 {
    id.as_str()
        .rsplit('-')
        .next()
        .and_then(|n| n.parse().ok())
        .unwrap_or(0)
}

fn receiving<'a>(
    st: &'a RouterState,
    to: &Address,
) -> impl Iterator<Item = (&'a SessionId, &'a SessionRecord)> {
    let to = to.clone();
    st.sessions.iter().filter(move |(_, r)| {
        r.space == to.space
            && !matches!(r.state, SessionState::Closed(_))
            && AgentRef::of(&r.actor) == Some(to.agent.clone())
    })
}

/// A message for the person, as their screen shows it: the words are theirs to read.
fn screen_line(message: &Message) -> docket_core::InboundLine {
    let mut line = inbound_line(message, |_| Handle(0));
    line.parts = message
        .parts
        .iter()
        .map(|p| match p {
            prov::Part::Text(t) => {
                docket_core::InboundPart::Text(docket_core::Reveal::Plain(t.as_str().to_owned()))
            }
            prov::Part::Entity(e) => docket_core::InboundPart::Entity(e.clone()),
            prov::Part::Outcome(o) => docket_core::InboundPart::Outcome(o.clone()),
            prov::Part::Undo(u) => docket_core::InboundPart::Undo(u.clone()),
        })
        .collect();
    line
}

fn refuse(why: SendRefusal) -> IntentsReply {
    IntentsReply::Refused(WireRefusal::Send(why))
}

impl<S: Seams> Router<S> {
    /// `.Message.Send`.
    pub(crate) fn message_send(
        &self,
        caller: &CallerId,
        role: CallerRole,
        session: &SessionId,
        draft: MessageDraft,
    ) -> IntentsReply {
        let now = self.seams.clock().now();
        let mut st = self.locked();
        let n = st.mint();
        let Some(record) = st.sessions.get(session) else {
            return IntentsReply::Refused(WireRefusal::NoSuchSession);
        };
        let person = matches!(role, CallerRole::Launcher | CallerRole::Field);
        let actor = if person {
            Actor::User {
                via: caller.app.name.clone(),
            }
        } else {
            record.actor.clone()
        };
        let Some(sender) = AgentRef::of(&actor) else {
            return refuse(SendRefusal::SenderMismatch);
        };
        if halted(&st.kill, &record.space).is_some()
            || (!person && matches!(record.state, SessionState::Paused { .. }))
        {
            return refuse(SendRefusal::Halted);
        }
        let mut handle_labels: Vec<Label> = Vec::new();
        let mut resolved: BTreeMap<Handle, MessageText> = BTreeMap::new();
        for part in &draft.parts {
            if let DraftPart::Handle(h) = part {
                match (record.handles.display(*h), record.handles.label(*h)) {
                    (Some(text), Some(label)) => {
                        resolved.insert(*h, MessageText::new(text));
                        handle_labels.push(label.clone());
                    }
                    _ => return refuse(SendRefusal::UnknownHandle),
                }
            }
        }
        let typed = draft.parts.iter().any(|p| matches!(p, DraftPart::Text(_)));
        let own = if person {
            vec![user_label()]
        } else {
            let words = typed.then(model_label);
            record.seen.iter().cloned().chain(words).collect()
        };
        let label = fold_labels(own.into_iter().chain(handle_labels)).unwrap_or_else(user_label);
        let (Ok(id), from) = (
            MessageId::parse(&format!("m-{n}")),
            Address::new(sender, record.space.clone()),
        ) else {
            return refuse(SendRefusal::Malformed(prov::Fault::NoParts));
        };
        let to = draft.to.clone();
        let message = match assemble(draft, id, from, label, now, &resolved)
            .and_then(|m| check_stamped(&m, &actor).map(|()| m))
        {
            Ok(m) => m,
            Err(why) => return refuse(why),
        };
        let Some(target) = receiving(&st, &to)
            .map(|(id, _)| id.clone())
            .max_by_key(age)
        else {
            return match to.agent {
                AgentRef::User => self.deliver_to_address(&mut st, to, message),
                _ => refuse(SendRefusal::NoRecipient),
            };
        };
        if let Some(r) = st.sessions.get_mut(&target) {
            absorb(r, &message.label);
            r.inbox.push(message.clone());
        }
        self.note_report(&mut st, session, &message);
        drop(st);
        self.seams
            .sink()
            .append(AuditRecord::Message(Box::new(message.clone())));
        IntentsReply::Delivered(Delivery {
            message: message.id.clone(),
            thread: message.thread.clone(),
            crossing: message.crossing(),
        })
    }

    fn deliver_to_address(
        &self,
        st: &mut RouterState,
        to: Address,
        message: Message,
    ) -> IntentsReply {
        st.inboxes.entry(to).or_default().push(message.clone());
        self.seams
            .sink()
            .append(AuditRecord::Message(Box::new(message.clone())));
        IntentsReply::Delivered(Delivery {
            message: message.id.clone(),
            thread: message.thread.clone(),
            crossing: message.crossing(),
        })
    }

    /// A report moves the sender's task: its final word ends it.
    fn note_report(&self, st: &mut RouterState, from: &SessionId, message: &Message) {
        let Some(status) = report_status(message) else {
            return;
        };
        let Some(task) = st.sessions.get(from).map(|r| r.task.clone()) else {
            return;
        };
        if let Some(t) = st.tasks.get_mut(&task) {
            t.state = match status {
                ReportStatus::Progress => TaskState::Working,
                done => TaskState::Ended(done),
            };
        }
    }

    /// `.Message.Inbox`: what waited for an agent, as its planner may read it. Reading takes
    /// the messages out.
    pub(crate) fn message_inbox(&self, role: CallerRole, ask: InboxAsk) -> IntentsReply {
        let allowed = matches!(
            (&ask.agent, role),
            (_, CallerRole::Launcher)
                | (
                    AgentRef::Companion | AgentRef::Worker { .. },
                    CallerRole::Companion
                )
                | (AgentRef::Cua { .. }, CallerRole::Cua)
        );
        if !allowed {
            return IntentsReply::Refused(WireRefusal::NotAllowed);
        }
        let mut st = self.locked();
        let ids: Vec<SessionId> = st
            .sessions
            .iter()
            .filter(|(_, r)| AgentRef::of(&r.actor) == Some(ask.agent.clone()))
            .map(|(id, _)| id.clone())
            .collect();
        let mut lines = Vec::new();
        for id in ids {
            let Some(record) = st.sessions.get_mut(&id) else {
                continue;
            };
            let waiting = std::mem::take(&mut record.inbox);
            let from = ask
                .after
                .as_ref()
                .and_then(|a| waiting.iter().position(|m| &m.id == a).map(|p| p + 1))
                .unwrap_or(0);
            for message in waiting.iter().skip(from) {
                let label = message.label.clone();
                let line = inbound_line(message, |text| {
                    record.handles.mint(
                        prov::Labelled {
                            value: crate::handles::HandleValue::Text(text.as_str().to_owned()),
                            label: label.clone(),
                        },
                        label
                            .sources
                            .iter()
                            .next()
                            .cloned()
                            .unwrap_or(prov::Source::User),
                    )
                });
                lines.push(line);
            }
        }
        for (_, waiting) in st.inboxes.iter_mut().filter(|(a, _)| a.agent == ask.agent) {
            for message in std::mem::take(waiting) {
                lines.push(screen_line(&message));
            }
        }
        IntentsReply::Inbox(lines)
    }
}
