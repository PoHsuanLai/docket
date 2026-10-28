//! The message checks, over the labels prov builds.

use docket_context::handles::{HandleTable, HandleValue};
use docket_core::*;
use docket_router::{assemble, check_stamped, inbound_line, intake_label, report_status};
use docket_vocab::Handle;
use porter_core::{AppName, DataClass};
use prov::{
    Actor, Address, AgentRef, AgentRole, Confidentiality, Crossing, Integrity, Label, Labelled,
    Message, MessageId, MessageKind, MessageText, Part, ReportStatus, SessionId, Source, SpaceId,
    TaskId, ThreadId, UnixSeconds,
};
use std::collections::{BTreeMap, BTreeSet};

fn space(s: &str) -> SpaceId {
    SpaceId::parse(s).expect("space")
}
fn app(s: &str) -> AppName {
    AppName::parse(s).expect("app")
}
fn trusted() -> Label {
    Label::trusted_user()
}
fn mail(in_space: &str) -> Label {
    Label::untrusted(Source::Mail, DataClass::Mail, space(in_space))
}

fn address(agent: AgentRef, in_space: &str) -> Address {
    Address::new(agent, space(in_space))
}

fn message(from: Address, to: Address, kind: MessageKind, label: Label) -> Message {
    Message {
        id: MessageId::parse("m-1").expect("id"),
        thread: ThreadId::parse("m-1").expect("thread"),
        in_reply_to: None,
        from,
        to,
        kind,
        parts: vec![Part::Text(MessageText::new("skip the newsletters"))],
        label,
        sent: UnixSeconds(5),
    }
}

fn worker() -> AgentRef {
    AgentRef::Worker {
        task: TaskId::parse("t-9").expect("task"),
    }
}

#[test]
fn the_router_checks_the_stamped_sender_against_the_actor() {
    let planner = Actor::Companion {
        session: SessionId::parse("s-1").expect("s"),
        role: AgentRole::Planner,
    };
    let person = Actor::User {
        via: app("org.quire.Shell"),
    };
    let from_companion = message(
        address(AgentRef::Companion, "work"),
        address(worker(), "work"),
        MessageKind::Request,
        trusted(),
    );
    assert_eq!(check_stamped(&from_companion, &planner), Ok(()));
    assert_eq!(
        check_stamped(&from_companion, &person),
        Err(SendRefusal::SenderMismatch)
    );
    let from_user = message(
        address(AgentRef::User, "work"),
        address(worker(), "work"),
        MessageKind::Request,
        trusted(),
    );
    assert_eq!(check_stamped(&from_user, &person), Ok(()));
    let user_report = message(
        address(AgentRef::User, "work"),
        address(worker(), "work"),
        MessageKind::Report {
            status: ReportStatus::Done,
        },
        trusted(),
    );
    assert_eq!(
        check_stamped(&user_report, &person),
        Err(SendRefusal::Malformed(prov::Fault::ReportFromUser))
    );
    let mut empty = from_companion.clone();
    empty.parts.clear();
    assert_eq!(
        check_stamped(&empty, &planner),
        Err(SendRefusal::Malformed(prov::Fault::NoParts))
    );
}

#[test]
fn a_draft_is_stamped_with_a_thread_and_its_handles_resolved() {
    let draft = MessageDraft {
        to: address(worker(), "home"),
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Request,
        parts: vec![
            DraftPart::Text(MessageText::new("look at this")),
            DraftPart::Handle(Handle(3)),
        ],
    };
    let resolved = BTreeMap::from([(Handle(3), MessageText::new("the mail body"))]);
    let id = MessageId::parse("m-7").expect("id");
    let stamped = assemble(
        draft.clone(),
        id.clone(),
        address(AgentRef::Companion, "work"),
        mail("work"),
        UnixSeconds(9),
        &resolved,
    )
    .expect("assembled");
    assert_eq!(stamped.thread.as_str(), "m-7");
    assert_eq!(stamped.parts.len(), 2);
    assert_eq!(stamped.crossing(), Crossing::Across);
    assert_eq!(stamped.label, mail("work"));
    let unknown = assemble(
        draft,
        id,
        address(AgentRef::Companion, "work"),
        trusted(),
        UnixSeconds(9),
        &BTreeMap::new(),
    );
    assert_eq!(unknown, Err(SendRefusal::UnknownHandle));
}

#[test]
fn a_message_from_an_untrusted_sender_lands_as_a_handle() {
    let untrusted = message(
        address(worker(), "home"),
        address(AgentRef::Companion, "work"),
        MessageKind::Report {
            status: ReportStatus::Done,
        },
        mail("home"),
    );
    let mut table = HandleTable::new();
    let line = inbound_line(&untrusted, |t| {
        table.mint(
            Labelled {
                value: HandleValue::Text(t.as_str().to_owned()),
                label: untrusted.label.clone(),
            },
            Source::Mail,
        )
    });
    assert_eq!(line.crossing, Crossing::Across);
    assert!(
        matches!(&line.parts[0], InboundPart::Text(Reveal::Handle(_))),
        "{line:?}"
    );
    let json = serde_json::to_string(&line).expect("json");
    assert!(!json.contains("newsletters"), "{json}");
    let own = message(
        address(AgentRef::User, "work"),
        address(worker(), "work"),
        MessageKind::Request,
        trusted(),
    );
    let line = inbound_line(&own, |_| Handle(0));
    assert_eq!(
        line.parts,
        [InboundPart::Text(Reveal::Plain(
            "skip the newsletters".into()
        ))]
    );
    assert_eq!(line.crossing, Crossing::Within);
    assert_eq!(report_status(&untrusted), Some(ReportStatus::Done));
    assert_eq!(report_status(&own), None);
}

#[test]
fn delivery_joins_the_message_label_into_the_receivers_taint() {
    let note = message(
        address(worker(), "home"),
        address(AgentRef::Companion, "work"),
        MessageKind::Note,
        mail("home"),
    );
    let joined = intake_label(&trusted(), &note);
    assert_eq!(joined.integrity, Integrity::Untrusted);
    assert_eq!(
        joined.confidentiality,
        Confidentiality::Private(BTreeSet::from([space("home")]))
    );
}
