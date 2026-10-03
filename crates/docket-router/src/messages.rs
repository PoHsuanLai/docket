//! Messages through the router. A message is `prov::Message`; this module is the pure half of
//! stamping and delivering one. A message carries no authority: delivery adds to the receiver's
//! input and joins its taint, and widens nothing.

use docket_core::{DraftPart, InboundLine, InboundPart, MessageDraft, Reveal, SendRefusal};
use prov::{
    Actor, Address, Label, Message, MessageId, MessageText, Part, ReportStatus, SenderCheck,
    ThreadId, UnixSeconds,
};
use std::collections::BTreeMap;

/// Checks a stamped message the way the router does before it delivers: well formed, and sent
/// by the party the transport says. The person may send; only agents report.
pub fn check_stamped(message: &Message, actor: &Actor) -> Result<(), SendRefusal> {
    message.check().map_err(SendRefusal::Malformed)?;
    match message.sender_matches(actor) {
        SenderCheck::Matches => Ok(()),
        SenderCheck::Mismatch => Err(SendRefusal::SenderMismatch),
    }
}

/// The label a receiver holds after taking a message in: the join of its own and the message's.
/// It never gets lower.
pub fn intake_label(receiver: &Label, message: &Message) -> Label {
    receiver.join(&message.label)
}

/// Builds the stamped message from a draft: the router supplies the id, the sender, the label
/// (the join of the sender's and of every handle it resolved) and the time; `resolved` holds
/// the text of each handle the draft names. A thread that is not given starts at this message.
pub fn assemble(
    draft: MessageDraft,
    id: MessageId,
    from: Address,
    label: Label,
    sent: UnixSeconds,
    resolved: &BTreeMap<docket_core::Handle, MessageText>,
) -> Result<Message, SendRefusal> {
    let thread = match draft.thread {
        Some(thread) => thread,
        None => ThreadId::parse(id.as_str())
            .map_err(|_| SendRefusal::Malformed(prov::Fault::NoParts))?,
    };
    let parts = draft
        .parts
        .into_iter()
        .map(|p| match p {
            DraftPart::Text(t) => Ok(Part::Text(t)),
            DraftPart::Handle(h) => resolved
                .get(&h)
                .cloned()
                .map(Part::Text)
                .ok_or(SendRefusal::UnknownHandle),
            DraftPart::Entity(e) => Ok(Part::Entity(e)),
            DraftPart::Outcome(o) => Ok(Part::Outcome(o)),
            DraftPart::Undo(u) => Ok(Part::Undo(u)),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Message {
        id,
        thread,
        in_reply_to: draft.in_reply_to,
        from,
        to: draft.to,
        kind: draft.kind,
        parts,
        label,
        sent,
    })
}

/// A delivered message as the receiving planner may read it: plain when the label is trusted,
/// a handle (minted through `mint`) when it is not.
pub fn inbound_line(
    message: &Message,
    mut mint: impl FnMut(MessageText) -> docket_core::Handle,
) -> InboundLine {
    let trusted = message.label.integrity == prov::Integrity::Trusted;
    let parts = message
        .parts
        .iter()
        .map(|p| match p {
            Part::Text(t) if trusted => InboundPart::Text(Reveal::Plain(t.as_str().to_owned())),
            Part::Text(t) => InboundPart::Text(Reveal::Handle(mint(t.clone()))),
            Part::Entity(e) => InboundPart::Entity(e.clone()),
            Part::Outcome(o) => InboundPart::Outcome(o.clone()),
            Part::Undo(u) => InboundPart::Undo(u.clone()),
        })
        .collect();
    InboundLine {
        id: message.id.clone(),
        thread: message.thread.clone(),
        to: message.to.clone(),
        from: message.from.clone(),
        crossing: message.crossing(),
        kind: message.kind,
        parts,
    }
}

/// The report status a final message carries, if it is a report.
pub fn report_status(message: &Message) -> Option<ReportStatus> {
    match message.kind {
        prov::MessageKind::Report { status } => Some(status),
        prov::MessageKind::Note | prov::MessageKind::Request => None,
    }
}
