//! The audit trail from the router to memoryd: each record becomes an almanac `Record` that
//! memoryd admits, the queue is bounded, and when memoryd is away nothing is lost until the
//! queue is full and nothing is written twice. The bus is a private one and memoryd is the
//! fake that serves the real wire.

mod support;

use almanac_client::DbusTransport;
use almanac_core::{AreaTag, Cause, EventBody, MemoryReply, MemoryRequest, Refusal};
use docket_core::*;
use docket_router::EventSink;
use intentd::{AuditLog, QueuedSink, kind_tag_of, record_of};
use porter_core::AppName;
use prov::{
    Actor, Address, AgentRef, ConfirmId, Effect, EntityId, EntityKey, EntityKind, Label, MessageId,
    MessageKind, MessageText, SessionId, SpaceId, SpaceScope, SystemPart, TaskId, ThreadId,
    UnixSeconds,
};
use support::bus::PrivateBus;
use support::memoryd::{FakeMemoryd, record_fault};

fn space(id: &str) -> SpaceId {
    SpaceId::parse(id).expect("space")
}

fn at(n: i64) -> UnixSeconds {
    UnixSeconds(n)
}

fn thread(key: &str) -> EntityId {
    EntityId {
        app: AppName::parse("org.quire.Mail").expect("app"),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn planner() -> Actor {
    Actor::Companion {
        session: SessionId::parse("s-4").expect("session"),
        role: prov::AgentRole::Planner,
    }
}

fn call(n: u64, in_space: &str) -> AuditRecord {
    AuditRecord::Call {
        at: at(10 + n as i64),
        call: CallId(n),
        actor: planner(),
        action: ActionRef {
            app: AppName::parse("org.quire.Mail").expect("app"),
            name: prov::ActionName::parse("mail.thread.archive").expect("action"),
        },
        targets: vec![thread("t1"), thread("t2")],
        effect: Effect::UndoableWrite,
        space: space(in_space),
        decided: DecidedBy::Policy(vec![PolicyId("grid".into())]),
        end: CallEnd::Done,
    }
}

fn halt(n: i64) -> AuditRecord {
    AuditRecord::Halt {
        at: at(n),
        scope: SpaceScope::Any,
        cause: HaltCause::KillChord,
    }
}

fn message() -> AuditRecord {
    AuditRecord::Message(Box::new(prov::Message {
        id: MessageId::parse("m-1").expect("id"),
        thread: ThreadId::parse("m-1").expect("thread"),
        in_reply_to: None,
        from: Address::new(AgentRef::Companion, space("work")),
        to: Address::new(
            AgentRef::Worker {
                task: TaskId::parse("t-9").expect("task"),
            },
            space("work"),
        ),
        kind: MessageKind::Request,
        parts: vec![prov::Part::Text(MessageText::new("find the receipts"))],
        label: Label::untrusted(
            prov::Source::Model(prov::ModelRole::Planner),
            porter_core::DataClass::Prompt,
            space("work"),
        ),
        sent: at(30),
    }))
}

fn episode() -> AuditRecord {
    use almanac_core::{Episode, EpisodeId, EpisodeKind, EpisodeOutcome, Skeleton};
    AuditRecord::Episode(Box::new(Episode {
        id: EpisodeId::parse("t-9").expect("episode"),
        agent: AgentRef::Worker {
            task: TaskId::parse("t-9").expect("task"),
        },
        kind: EpisodeKind::Task,
        parent: None,
        space: space("work"),
        started: at(31),
        ended: at(40),
        outcome: EpisodeOutcome::Done,
        skeleton: Skeleton {
            label: Label::trusted_user(),
            asked: vec![],
            steps: vec![],
            touched: vec![],
            results: vec![],
        },
        narrative: None,
    }))
}

/// One of every kind of record the router writes.
fn everything() -> Vec<AuditRecord> {
    vec![
        call(1, "work"),
        AuditRecord::Review {
            at: at(11),
            call: CallId(1),
            mark: ReviewMark {
                stage: Stage::Quick,
                verdict: Ok(VerdictKind::Allow),
                code: ReasonCode::WithinRequest,
                latency: Millis(90),
                model: porter_core::ModelId::parse("holo").expect("model"),
            },
        },
        AuditRecord::Confirm {
            at: at(12),
            id: ConfirmId::parse("c-1").expect("id"),
            answer: ConfirmAnswerKind::Ended(ConfirmEnd::Refused),
            input: None,
        },
        AuditRecord::Undo {
            at: at(13),
            entry: UndoId(1),
            by: Actor::Cli,
            end: UndoState::Undone { by: Actor::Cli },
        },
        halt(14),
        AuditRecord::TaskPolicy {
            at: at(15),
            task: TaskId::parse("t-1").expect("task"),
            state: TaskPolicyState::Active,
            change: PolicyChangeKind::Same,
        },
        AuditRecord::Breaker {
            at: at(16),
            session: SessionId::parse("s-4").expect("session"),
            trip: BreakerTrip::Consecutive,
        },
        AuditRecord::TaskStarted {
            at: at(17),
            task: TaskId::parse("t-9").expect("task"),
            agent: AgentRef::Worker {
                task: TaskId::parse("t-9").expect("task"),
            },
            parent: None,
            space: space("work"),
            by_call: CallId(1),
        },
        message(),
        episode(),
    ]
}

#[test]
fn every_record_the_router_writes_becomes_a_record_memoryd_admits() {
    for record in everything() {
        let event = record_of(&record, &space("desktop"));
        assert_eq!(
            record_fault(&event),
            None,
            "memoryd would refuse {:?}",
            kind_tag_of(&record)
        );
        assert_eq!(event.cause, Cause::None);
        assert_eq!(
            event.body.kind().as_str(),
            kind_tag_of(&record).expect("a kind").as_str(),
            "the header kind is the record's own"
        );
    }
}

#[test]
fn a_call_is_an_area_payload_that_names_what_it_touched_and_reads_back_whole() {
    let record = call(3, "work");
    let event = record_of(&record, &space("desktop"));
    assert_eq!(event.space, space("work"), "the Space the call names wins");
    assert_eq!(event.occurred, at(13));
    assert_eq!(event.actor, planner());
    assert_eq!(event.effect, Effect::UndoableWrite);
    let EventBody::Area(payload) = &event.body else {
        panic!("{:?}", event.body)
    };
    assert_eq!(payload.area, AreaTag::Docket);
    assert_eq!(payload.kind.as_str(), "docket.call");
    let named: Vec<&EntityId> = payload.things.iter().map(|(t, _)| &t.thing).collect();
    assert_eq!(
        named,
        [&thread("t1"), &thread("t2")],
        "cascade-forget finds the call by its things"
    );
    assert!(
        payload
            .things
            .iter()
            .all(|(view, _)| view.title.as_str().is_empty()),
        "ids, never titles"
    );
    let back: AuditRecord = serde_json::from_str(payload.json.as_str()).expect("the serde form");
    assert_eq!(back, record);
}

#[test]
fn a_record_that_names_no_space_goes_where_the_caller_puts_it() {
    let event = record_of(&halt(5), &space("home"));
    assert_eq!(event.space, space("home"));
    assert_eq!(
        event.actor,
        Actor::System {
            part: SystemPart::Router
        }
    );
    assert_eq!(event.effect, Effect::Read);
}

#[test]
fn a_message_and_an_episode_keep_almanacs_own_typed_bodies() {
    let sent = record_of(&message(), &space("desktop"));
    let EventBody::Message(body) = &sent.body else {
        panic!("{:?}", sent.body)
    };
    assert_eq!(body.text(), "find the receipts");
    assert_eq!(sent.label, body.label, "the message's own label");
    assert_eq!(sent.space, space("work"), "the sender's Space");
    assert_eq!(sent.actor, record_of(&message(), &space("x")).actor);
    let done = record_of(&episode(), &space("desktop"));
    assert!(matches!(done.body, EventBody::Episode(_)));
    assert_eq!(done.occurred, at(40));
}

#[test]
fn the_queue_is_bounded_drops_the_oldest_and_counts_them() {
    let sink = QueuedSink::bounded(3);
    (1..=5).for_each(|n| sink.append(halt(n)));
    assert_eq!(sink.len(), 3);
    assert_eq!(sink.dropped(), 2);
    assert_eq!(sink.dropped(), 0, "counted once");
    let held = sink.drain();
    assert!(
        matches!(
            held[0],
            AuditRecord::Halt {
                at: UnixSeconds(3),
                ..
            }
        ),
        "the oldest went: {held:?}"
    );
    assert!(sink.is_empty());
}

#[test]
fn what_could_not_be_written_goes_back_in_front_of_what_came_since() {
    let sink = QueuedSink::bounded(4);
    sink.append(halt(1));
    sink.append(halt(2));
    let taken = sink.drain();
    sink.append(halt(3));
    sink.restore(taken);
    let order: Vec<i64> = sink
        .drain()
        .iter()
        .map(|r| match r {
            AuditRecord::Halt { at, .. } => at.0,
            other => panic!("{other:?}"),
        })
        .collect();
    assert_eq!(order, [1, 2, 3]);

    // Restoring past the limit drops the oldest and counts them.
    let sink = QueuedSink::bounded(2);
    sink.append(halt(9));
    sink.restore(vec![halt(1), halt(2), halt(3)]);
    assert_eq!(sink.dropped(), 2);
    assert_eq!(sink.len(), 2);
}

fn times(records: &[almanac_core::Record]) -> Vec<i64> {
    records.iter().map(|r| r.occurred.0).collect()
}

#[tokio::test]
async fn audit_records_reach_memoryd_by_space_in_order_and_the_queue_empties() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let memoryd = FakeMemoryd::recording();
    let server = bus.connect().await;
    memoryd.serve(&server).await;
    let client = bus.connect().await;
    let mut log = AuditLog::over(DbusTransport::new(client));
    let sink = QueuedSink::new();
    sink.append(call(1, "work"));
    sink.append(call(2, "home"));
    sink.append(halt(3));
    sink.append(call(4, "work"));

    let report = log.flush(&sink, |_| None).await;

    assert_eq!((report.written, report.waiting, report.lost), (4, 0, 0));
    assert!(sink.is_empty());
    let stored = memoryd.stored();
    let by_space = |name: &str| {
        let s = space(name);
        stored
            .iter()
            .filter(|r| r.space == s)
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(
        times(&by_space("work")),
        [11, 14],
        "each Space in the order it was queued"
    );
    assert_eq!(times(&by_space("home")), [12]);
    assert_eq!(
        times(&by_space("desktop")),
        [3],
        "a record with no Space is the desktop's"
    );
    assert!(
        memoryd
            .seen()
            .iter()
            .all(|r| matches!(r, MemoryRequest::RecordBatch(_))),
        "one batch per Space"
    );
}

#[tokio::test]
async fn a_review_lands_in_the_space_of_its_call_and_a_breaker_where_the_router_says() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let memoryd = FakeMemoryd::recording();
    let server = bus.connect().await;
    memoryd.serve(&server).await;
    let mut log = AuditLog::over(DbusTransport::new(bus.connect().await));
    let sink = QueuedSink::new();
    let records = everything();
    // The call is recorded in an earlier pass than its review.
    sink.append(call(1, "work"));
    log.flush(&sink, |_| None).await;
    sink.append(records[1].clone());
    sink.append(records[6].clone());
    let report = log
        .flush(&sink, |r| match r {
            AuditRecord::Breaker { .. } => Some(space("home")),
            _ => None,
        })
        .await;
    assert_eq!(report.written, 2);
    let stored = memoryd.stored();
    let review = stored
        .iter()
        .find(|r| r.body.kind().as_str() == "docket.review")
        .expect("review");
    let breaker = stored
        .iter()
        .find(|r| r.body.kind().as_str() == "docket.breaker")
        .expect("breaker");
    assert_eq!(
        (review.space.clone(), breaker.space.clone()),
        (space("work"), space("home"))
    );
}

#[tokio::test]
async fn while_memoryd_is_away_nothing_is_lost_and_nothing_is_written_twice() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let client = bus.connect().await;
    let mut log = AuditLog::over(DbusTransport::new(client));
    let sink = QueuedSink::new();
    sink.append(call(1, "work"));
    sink.append(call(2, "home"));

    let away = log.flush(&sink, |_| None).await;
    assert_eq!((away.written, away.waiting, away.lost), (0, 2, 0));
    assert_eq!(sink.len(), 2, "they wait for it");

    sink.append(call(3, "work"));
    let still = log.flush(&sink, |_| None).await;
    assert_eq!((still.written, still.waiting), (0, 3));

    let memoryd = FakeMemoryd::recording();
    let server = bus.connect().await;
    memoryd.serve(&server).await;
    let back = log.flush(&sink, |_| None).await;
    assert_eq!((back.written, back.waiting, back.lost), (3, 0, 0));
    assert_eq!(
        times(&memoryd.stored()),
        [11, 13, 12].map(|n| n),
        "work's two in order, then home's: {:?}",
        times(&memoryd.stored())
    );
    let again = log.flush(&sink, |_| None).await;
    assert_eq!(again.written, 0, "never written twice");
}

#[tokio::test]
async fn a_memoryd_that_is_never_there_costs_a_bounded_queue_and_a_count_of_what_was_dropped() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let mut log = AuditLog::over(DbusTransport::new(bus.connect().await));
    let sink = QueuedSink::bounded(3);
    (1..=5).for_each(|n| sink.append(call(n, "work")));
    // Five records went in a queue of three: two were dropped before the pass.
    let report = log.flush(&sink, |_| None).await;
    assert_eq!((report.written, report.waiting, report.lost), (0, 3, 2));
    assert_eq!(sink.len(), 3);
}

#[tokio::test]
async fn a_record_memoryd_refuses_for_good_is_dropped_and_the_rest_are_kept() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let memoryd = FakeMemoryd::with(|request| {
        let bad = |r: &almanac_core::Record| r.body.kind().as_str() == "docket.breaker";
        match request {
            MemoryRequest::RecordBatch(rs) if rs.iter().any(bad) => {
                Some(MemoryReply::Refused(Refusal::Invalid("no".into())))
            }
            MemoryRequest::Record(r) if bad(r) => {
                Some(MemoryReply::Refused(Refusal::Invalid("no".into())))
            }
            _ => None,
        }
    });
    let server = bus.connect().await;
    memoryd.serve(&server).await;
    let mut log = AuditLog::over(DbusTransport::new(bus.connect().await));
    let sink = QueuedSink::new();
    let records = everything();
    sink.append(records[6].clone()); // the breaker, desktop
    sink.append(records[4].clone()); // a halt, desktop
    let report = log.flush(&sink, |_| None).await;
    assert_eq!((report.written, report.waiting, report.lost), (1, 0, 1));
    assert_eq!(memoryd.stored().len(), 1);
    assert!(sink.is_empty(), "a refused record is not retried for ever");
}

#[tokio::test]
async fn a_locked_space_waits_and_is_tried_again() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let locked = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(1));
    let flag = locked.clone();
    let memoryd = FakeMemoryd::with(move |request| match request {
        MemoryRequest::RecordBatch(_) if flag.load(std::sync::atomic::Ordering::SeqCst) == 1 => {
            Some(MemoryReply::Refused(Refusal::SpaceLocked))
        }
        _ => None,
    });
    let server = bus.connect().await;
    memoryd.serve(&server).await;
    let mut log = AuditLog::over(DbusTransport::new(bus.connect().await));
    let sink = QueuedSink::new();
    sink.append(call(1, "work"));
    let first = log.flush(&sink, |_| None).await;
    assert_eq!((first.written, first.waiting), (0, 1));
    locked.store(0, std::sync::atomic::Ordering::SeqCst);
    let second = log.flush(&sink, |_| None).await;
    assert_eq!((second.written, second.waiting), (1, 0));
}
