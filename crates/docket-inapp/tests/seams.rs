//! The in-app agent with real parts instead of the stubs: a write judged by a model-backed
//! reviewer, a `quire_read` answered by the portable reader, recall and the audit trail through
//! almanac's in-process memory, and consent that outlives an `InAppAgent`. Every model is a
//! scripted transport, the memory is almanac-fake's backend, every file is in a scratch directory
//! and the clock is virtual.

mod support;

use almanac_client::{InProcess, Memory};
use almanac_core::{
    BodyMode, Caller, FactDraft, FactText, MemoryReply, MemoryRequest, RecentQuery, TopicPath,
    TrustFilter,
};
use almanac_fake::{ScriptedConsolidator, fake_service};
use docket_core::{AgentConfig, ChoiceId, Handle, ReaderAsk, ReaderTask, StepEnd, ValueSchema};
use docket_fake::{FakeMail, FixedClock, MailThread, ScriptedReviewer, mail_manifest};
use docket_inapp::{
    AlmanacMemory, AuditTo, FileGrantStore, InAppAgent, InAppKit, InAppParts, TransportReader,
    TransportWriter, reviewer_over,
};
use porter_core::Count;
use prov::{SpaceId, UnixSeconds};
use serde_json::json;
use std::sync::Arc;
use support::infer::{ScriptedInfer, call, words};
use support::{Nowhere, TestSheet, standing_mail_consent};

const ARCHIVE: &str = "org.quire.Mail-mail.thread.archive";
const READ: &str = "org.quire.Mail-mail.thread.read";

fn work() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn thread(key: &str) -> serde_json::Value {
    json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

fn mail() -> FakeMail {
    let mail = FakeMail::new(mail_manifest().expect("manifest"), work());
    for (key, subject) in [("t1", "Invoice"), ("t2", "Digest")] {
        mail.add_thread(MailThread {
            key: key.into(),
            subject: subject.into(),
            from: "news@example.test".into(),
            body: "This week".into(),
        });
    }
    mail
}

fn parts<R>(
    reviewer: R,
    model: &ScriptedInfer,
    sheet: &TestSheet,
) -> InAppParts<FakeMail, Nowhere, TestSheet, R, ScriptedInfer, FixedClock> {
    InAppParts {
        provider: mail(),
        context: Nowhere,
        sheet: sheet.clone(),
        reviewer,
        model: model.clone(),
        clock: FixedClock::at(UnixSeconds(1_000)),
        space: work(),
        config: AgentConfig::default(),
    }
}

/// The policy a writer drafts for "archive the digest": the archive action, up to an undoable write.
fn archive_draft() -> String {
    json!({
        "actions": ["org.quire.Mail mail.thread.archive"], "apps": [], "kinds": [],
        "ceiling": "undoable_write", "max_count": 1,
        "recipients": [], "destinations": [], "paths": [],
    })
    .to_string()
}

/// A reviewer cascade over three scripted transports, one per stage.
fn cascade(
    stages: [&ScriptedInfer; 3],
) -> action_review::InferReviewer<docket_inapp::TransportModel<ScriptedInfer>> {
    let mut stages = stages.into_iter().cloned();
    reviewer_over(None, AgentConfig::default().review, move || {
        stages.next().expect("three stages")
    })
    .expect("a cascade")
}

#[tokio::test]
async fn a_write_the_model_reviewer_passes_runs_without_asking() {
    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Archived the digest."),
    ]);
    let writer = ScriptedInfer::new(vec![words(&archive_draft())]);
    let (quick, deliberate, second) = (
        ScriptedInfer::new(vec![words("pass")]),
        ScriptedInfer::default(),
        ScriptedInfer::default(),
    );
    let sheet = TestSheet::default();
    let mut agent = InAppAgent::with_kit(
        parts(cascade([&quick, &deliberate, &second]), &planner, &sheet),
        InAppKit::default()
            .grants(standing_mail_consent())
            .writer(TransportWriter::new(writer.clone())),
    )
    .expect("agent");
    let reply = agent.ask("archive the digest").await.expect("turn");
    assert!(
        matches!(reply.steps[0].end, StepEnd::Done { .. }),
        "{reply:?}"
    );
    assert!(agent.provider().is_archived("t2"));
    assert!(
        sheet.shown().is_empty(),
        "the policy and the reviewer decided"
    );
    assert_eq!(
        writer.asked().len(),
        1,
        "the policy was derived from the person's words"
    );
    assert_eq!(quick.asked().len(), 1, "the quick judge looked once");
    assert!(deliberate.asked().is_empty(), "a pass does not escalate");
    let seen = quick.user_text(0);
    assert!(
        !seen.contains("This week") && !seen.contains("news@example.test"),
        "the reviewer reads the typed action, never the content: {seen}"
    );
}

#[tokio::test]
async fn a_flag_escalates_to_the_larger_model_and_its_ask_reaches_the_sheet() {
    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Archived the digest."),
    ]);
    let writer = ScriptedInfer::new(vec![words(&archive_draft())]);
    let quick = ScriptedInfer::new(vec![words("flag")]);
    let deliberate = ScriptedInfer::new(vec![words(
        r#"{"verdict":"ask","code":"uncertain","reason":"not sure the person meant this one"}"#,
    )]);
    let second = ScriptedInfer::default();
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent = InAppAgent::with_kit(
        parts(cascade([&quick, &deliberate, &second]), &planner, &sheet),
        InAppKit::default()
            .grants(standing_mail_consent())
            .writer(TransportWriter::new(writer)),
    )
    .expect("agent");
    let reply = agent.ask("archive the digest").await.expect("turn");
    assert!(
        matches!(reply.steps[0].end, StepEnd::Done { .. }),
        "{reply:?}"
    );
    assert_eq!(
        deliberate.asked().len(),
        1,
        "the flag brought in the larger model"
    );
    assert!(
        !sheet.shown().is_empty(),
        "an ask is the person's to answer"
    );
}

#[tokio::test]
async fn a_model_reviewer_that_cannot_answer_asks_the_person_and_never_allows() {
    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Left it."),
    ]);
    let writer = ScriptedInfer::new(vec![words(&archive_draft())]);
    // Nothing scripted: every stage's model gives nothing.
    let (quick, deliberate, second) = Default::default();
    let sheet = TestSheet::default();
    let mut agent = InAppAgent::with_kit(
        parts(cascade([&quick, &deliberate, &second]), &planner, &sheet),
        InAppKit::default()
            .grants(standing_mail_consent())
            .writer(TransportWriter::new(writer)),
    )
    .expect("agent");
    let reply = agent.ask("archive the digest").await.expect("turn");
    assert!(!agent.provider().is_archived("t2"));
    assert!(
        !matches!(reply.steps[0].end, StepEnd::Done { .. }),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_quire_read_is_answered_by_the_portable_reader_and_the_planner_sees_only_the_answer() {
    let ask = ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Choice(vec![
            ChoiceId::parse("invoice").expect("choice"),
            ChoiceId::parse("newsletter").expect("choice"),
        ]),
        task: ReaderTask::Classify,
    };
    let planner = ScriptedInfer::new(vec![
        call(READ, json!({ "target": thread("t1") })),
        call("quire_read", serde_json::to_value(&ask).expect("ask")),
        words("It is an invoice."),
    ]);
    let reader = ScriptedInfer::new(vec![words("invoice")]);
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner, &sheet),
        InAppKit::default().reader(TransportReader::in_process(reader.clone())),
    )
    .expect("agent");
    let reply = agent
        .ask("what kind of mail is the invoice")
        .await
        .expect("turn");
    assert_eq!(reply.ending, docket_inapp::Ending::Done, "{reply:?}");
    assert_eq!(reader.asked().len(), 1);
    assert!(
        reader.user_text(0).contains("This week"),
        "the reader sees the data, fenced"
    );
    for n in 0..3 {
        assert!(
            !planner.user_text(n).contains("This week"),
            "the planner never reads it (request {n})"
        );
    }
    assert!(
        planner.user_text(2).contains("invoice"),
        "the planner is shown the typed answer: {}",
        planner.user_text(2)
    );
}

#[tokio::test]
async fn without_a_reader_the_read_ends_the_turn_as_failed() {
    let ask = ReaderAsk {
        inputs: vec![Handle(1)],
        want: ValueSchema::Choice(vec![ChoiceId::parse("invoice").expect("choice")]),
        task: ReaderTask::Classify,
    };
    let planner = ScriptedInfer::new(vec![
        call(READ, json!({ "target": thread("t1") })),
        call("quire_read", serde_json::to_value(&ask).expect("ask")),
    ]);
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent =
        InAppAgent::new(parts(ScriptedReviewer::always_allow(), &planner, &sheet)).expect("agent");
    let reply = agent.ask("classify the invoice").await.expect("turn");
    assert!(
        matches!(
            reply.ending,
            docket_inapp::Ending::Failed(docket_inapp::Failure::Reader)
        ),
        "{reply:?}"
    );
}

/// The router's memory over almanac-fake's service, hosted in this process.
macro_rules! router_memory {
    ($service:expr) => {
        AlmanacMemory::over(InProcess::new($service.clone(), Caller::Router))
    };
}

/// The person (the shell) tells memory a fact about the work Space.
macro_rules! teach {
    ($service:expr, $text:expr) => {{
        let shell = Memory::over(InProcess::new($service.clone(), Caller::ShellUi));
        shell
            .propose(
                work(),
                FactDraft {
                    topic: TopicPath::parse("people/ana").expect("topic"),
                    text: FactText::parse($text).expect("text"),
                    links: vec![],
                    supersedes: vec![],
                },
            )
            .await
            .expect("propose");
    }};
}

fn memory_kit<M>(memory: M) -> InAppKit<docket_inapp::SessionGrants, M> {
    InAppKit::default().memory(memory).audit_to(AuditTo::Memory)
}

#[tokio::test]
async fn recall_from_in_process_memory_reaches_the_planner() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    teach!(service, "Ana sent the budget report.");
    let planner = ScriptedInfer::new(vec![words("Noted.")]);
    let sheet = TestSheet::default();
    let mut agent = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner, &sheet),
        memory_kit(router_memory!(service)),
    )
    .expect("agent");
    let reply = agent
        .ask("what did Ana say about the budget")
        .await
        .expect("turn");
    assert_eq!(reply.ending, docket_inapp::Ending::Done, "{reply:?}");
    let view = planner.user_text(0);
    assert!(
        view.contains("Ana sent the budget report."),
        "recall reached the planner's view: {view}"
    );
}

#[tokio::test]
async fn the_turns_audit_is_written_as_records_and_the_task_is_an_episode_the_next_agent_recalls() {
    let service = Arc::new(fake_service(ScriptedConsolidator::default()));
    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Archived the digest."),
    ]);
    let sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Once; 2]);
    let mut agent = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner, &sheet),
        memory_kit(router_memory!(service)),
    )
    .expect("agent");
    agent.ask("archive the digest").await.expect("turn");
    assert!(
        agent.audit().is_empty(),
        "everything was written: {:?}",
        agent.audit()
    );

    let reader = router_memory!(service);
    let recent = docket_router::MemoryLink::ask(
        &reader,
        MemoryRequest::Recent(
            work(),
            RecentQuery {
                since: UnixSeconds(0),
                kinds: Vec::new(),
                trust: TrustFilter::Any,
                limit: Count(50),
                bodies: BodyMode::Json,
            },
        ),
    )
    .await
    .expect("recent");
    let MemoryReply::Recent(entries) = recent else {
        panic!("recent: {recent:?}");
    };
    let kinds: Vec<String> = entries
        .iter()
        .map(|e| format!("{:?}", e.summary.kind))
        .collect();
    for want in ["docket.call", "companion.episode"] {
        assert!(
            kinds.iter().any(|k| k.contains(want)),
            "{want} among {kinds:?}"
        );
    }

    // A later agent over the same memory is shown the earlier task.
    let planner2 = ScriptedInfer::new(vec![words("Hello again.")]);
    let sheet2 = TestSheet::default();
    let mut again = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner2, &sheet2),
        memory_kit(router_memory!(service)),
    )
    .expect("agent");
    again.ask("hello").await.expect("turn");
    assert!(
        planner2.user_text(0).contains("archive the digest"),
        "the earlier episode is in the view: {}",
        planner2.user_text(0)
    );
}

#[tokio::test]
async fn an_always_grant_outlives_the_agent_that_was_given_it() {
    let dir = tempfile::tempdir().expect("scratch");
    let file = dir.path().join("grants.json");
    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t2")] })),
        words("Archived the digest."),
    ]);
    let first_sheet = TestSheet::answering(vec![docket_inapp::SheetAnswer::Always; 2]);
    let mut first = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner, &first_sheet),
        InAppKit::default().grants(FileGrantStore::open(&file).expect("grants")),
    )
    .expect("agent");
    first.ask("archive the digest").await.expect("turn");
    assert!(first.provider().is_archived("t2"));
    assert!(
        !first_sheet.shown().is_empty(),
        "the first time the person was asked"
    );
    drop(first);

    let planner = ScriptedInfer::new(vec![
        call(ARCHIVE, json!({ "target": [thread("t1")] })),
        words("Archived the invoice."),
    ]);
    // The second sheet answers nothing: an unanswered sheet is a dismissal.
    let second_sheet = TestSheet::default();
    let mut second = InAppAgent::with_kit(
        parts(ScriptedReviewer::always_allow(), &planner, &second_sheet),
        InAppKit::default().grants(FileGrantStore::open(&file).expect("grants")),
    )
    .expect("agent");
    let reply = second.ask("archive the invoice").await.expect("turn");
    assert!(
        matches!(reply.steps[0].end, StepEnd::Done { .. }),
        "{reply:?}"
    );
    assert!(second.provider().is_archived("t1"));
    assert!(
        second_sheet.shown().is_empty(),
        "standing consent was not asked again"
    );
}
