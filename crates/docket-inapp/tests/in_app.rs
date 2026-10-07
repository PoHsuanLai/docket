//! The in-app agent end to end: the app's own provider (docket-fake's mail), a scripted model, a
//! sheet the test answers, a virtual clock. No intentd, no bus, no daemon, no socket.

// The scripted model of companiond's tests is the same transport: one file, not a copy.
#[allow(dead_code)]
#[path = "../../companiond/tests/support/infer.rs"]
mod infer;

use docket_client::ContextSource;
use docket_core::{
    AgentConfig, CallRefusal, ConfirmEnd, ConfirmId, ConfirmRequest, ContextScope, ContextSnapshot,
    Here, Selection, StepEnd, TextTarget, Visible, WindowPrivacy,
};
use docket_fake::{FakeMail, FixedClock, MailThread, ScriptedReviewer, mail_manifest};
use docket_inapp::{ConfirmSheet, Ending, InAppAgent, InAppParts, SheetAnswer};
use infer::{Say, ScriptedInfer, call, words};
use porter_core::Count;
use prov::{Labelled, SpaceId, UnixSeconds};
use serde_json::json;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// The sheet the test answers from a queue, remembering what it was shown.
#[derive(Debug, Default, Clone)]
struct TestSheet {
    answers: Arc<Mutex<VecDeque<SheetAnswer>>>,
    shown: Arc<Mutex<Vec<ConfirmRequest>>>,
}

impl TestSheet {
    fn answering(answers: Vec<SheetAnswer>) -> Self {
        Self {
            answers: Arc::new(Mutex::new(answers.into())),
            shown: Arc::default(),
        }
    }
    fn shown(&self) -> Vec<ConfirmRequest> {
        self.shown.lock().expect("lock").clone()
    }
}

impl ConfirmSheet for TestSheet {
    async fn ask(&self, request: &ConfirmRequest) -> SheetAnswer {
        self.shown.lock().expect("lock").push(request.clone());
        self.answers
            .lock()
            .expect("lock")
            .pop_front()
            .unwrap_or(SheetAnswer::Dismissed)
    }
    async fn withdraw(&self, _id: &ConfirmId) {}
}

/// What the person is looking at: nothing in particular.
#[derive(Debug, Clone, Copy)]
struct Nowhere;

impl ContextSource for Nowhere {
    fn snapshot(&self, _scope: ContextScope) -> ContextSnapshot {
        ContextSnapshot {
            app: porter_core::AppName::parse("org.quire.Mail").expect("app"),
            window: Labelled {
                value: String::new(),
                label: prov::Label::trusted_user(),
            },
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: vec![],
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        }
    }
}

type Agent = InAppAgent<FakeMail, Nowhere, TestSheet, ScriptedReviewer, ScriptedInfer, FixedClock>;

fn agent(script: Vec<Say>, sheet: TestSheet) -> (Agent, ScriptedInfer) {
    let space = SpaceId::parse("work").expect("space");
    let mail = FakeMail::new(mail_manifest().expect("manifest"), space.clone());
    for (key, subject) in [("t1", "Invoice"), ("t2", "Digest")] {
        mail.add_thread(MailThread {
            key: key.into(),
            subject: subject.into(),
            from: "news@example.test".into(),
            body: "This week".into(),
        });
    }
    let model = ScriptedInfer::new(script);
    let agent = InAppAgent::new(InAppParts {
        provider: mail,
        context: Nowhere,
        sheet,
        reviewer: ScriptedReviewer::always_allow(),
        model: model.clone(),
        clock: FixedClock::at(UnixSeconds(1_000)),
        space,
        config: AgentConfig::default(),
    })
    .expect("agent");
    (agent, model)
}

const ARCHIVE: &str = "org.quire.Mail-mail.thread.archive";
const READ: &str = "org.quire.Mail-mail.thread.read";

fn thread(key: &str) -> serde_json::Value {
    json!({ "app": "org.quire.Mail", "kind": "mail.thread", "key": key })
}

#[tokio::test]
async fn words_only_is_a_finished_turn_with_no_calls() {
    let (mut agent, model) = agent(vec![words("Nothing to do.")], TestSheet::default());
    let reply = agent.ask("hello").await.expect("turn");
    assert_eq!(reply.said, ["Nothing to do."]);
    assert!(reply.steps.is_empty());
    assert_eq!(reply.ending, Ending::Done);
    assert_eq!(model.remaining(), 0);
}

#[tokio::test]
async fn a_write_asks_on_the_apps_sheet_and_runs_when_the_person_says_yes() {
    let sheet = TestSheet::answering(vec![SheetAnswer::Once, SheetAnswer::Once]);
    let (mut agent, _) = agent(
        vec![
            call(ARCHIVE, json!({ "target": [thread("t2")] })),
            words("Archived the digest."),
        ],
        sheet.clone(),
    );
    let reply = agent.ask("archive the digest").await.expect("turn");
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    assert_eq!(reply.steps.len(), 1);
    assert!(
        matches!(reply.steps[0].end, StepEnd::Done { .. }),
        "{reply:?}"
    );
    assert!(agent.provider().is_archived("t2"));
    assert!(!sheet.shown().is_empty(), "the sheet was drawn by the app");
    assert!(!agent.audit().is_empty(), "the router audited the call");
}

#[tokio::test]
async fn a_no_on_the_sheet_means_the_app_never_runs_the_action() {
    let sheet = TestSheet::answering(vec![SheetAnswer::Refused, SheetAnswer::Refused]);
    let (mut agent, _) = agent(
        vec![
            call(ARCHIVE, json!({ "target": [thread("t2")] })),
            words("It was not allowed."),
        ],
        sheet,
    );
    let reply = agent.ask("archive the digest").await.expect("turn");
    assert!(!agent.provider().is_archived("t2"));
    assert!(
        matches!(
            reply.steps[0].end,
            StepEnd::Unconfirmed(ConfirmEnd::Refused)
                | StepEnd::Refused(CallRefusal::Unconfirmed(_))
        ),
        "{reply:?}"
    );
}

#[tokio::test]
async fn an_unanswered_sheet_is_a_dismissal_never_a_yes() {
    let (mut agent, _) = agent(
        vec![
            call(ARCHIVE, json!({ "target": [thread("t1")] })),
            words("Stopped."),
        ],
        TestSheet::default(),
    );
    let reply = agent.ask("archive it").await.expect("turn");
    assert!(!agent.provider().is_archived("t1"));
    assert!(
        matches!(reply.steps[0].end, StepEnd::Unconfirmed(_)),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_read_runs_without_asking_and_the_text_reaches_the_planner_only_as_a_handle() {
    let sheet = TestSheet::answering(vec![SheetAnswer::Once]);
    let (mut agent, model) = agent(
        vec![
            call(READ, json!({ "target": thread("t1") })),
            words("Read it."),
        ],
        sheet,
    );
    let reply = agent.ask("what does the invoice say").await.expect("turn");
    assert_eq!(reply.ending, Ending::Done, "{reply:?}");
    let second = model.user_text(1);
    assert!(
        !second.contains("This week"),
        "untrusted text stays behind a handle: {second}"
    );
}

#[tokio::test]
async fn a_question_ends_the_turn_and_the_next_ask_goes_on_in_the_same_task() {
    let (mut agent, model) = agent(
        vec![
            infer::call(
                "quire_ask",
                json!({ "text": "Which one?", "choices": ["a", "b"] }),
            ),
            words("Fine."),
        ],
        TestSheet::default(),
    );
    let first = agent.ask("tidy up").await.expect("turn");
    assert_eq!(
        first.ending,
        Ending::Asked {
            text: "Which one?".into(),
            choices: vec!["a".into(), "b".into()]
        }
    );
    let second = agent.ask("a").await.expect("turn");
    assert_eq!(second.ending, Ending::Done);
    assert!(
        model.user_text(1).contains("tidy up"),
        "the task's turns carried over"
    );
}

#[tokio::test]
async fn a_model_that_gives_nothing_usable_fails_the_turn_without_touching_the_app() {
    let (mut agent, _) = agent(vec![], TestSheet::default());
    let reply = agent.ask("anything").await.expect("turn");
    assert!(matches!(reply.ending, Ending::Failed(_)), "{reply:?}");
    assert!(reply.steps.is_empty());
}

#[tokio::test]
async fn a_call_the_person_dismissed_is_not_made_again_by_asking_again() {
    let same = || call(READ, json!({ "target": thread("t1") }));
    let (mut agent, _) = agent(
        vec![same(), same(), words("Giving up.")],
        TestSheet::default(),
    );
    let reply = agent.ask("read it").await.expect("turn");
    assert!(
        matches!(reply.steps[0].end, StepEnd::Unconfirmed(_)),
        "{reply:?}"
    );
    assert!(
        matches!(
            reply.steps[1].end,
            StepEnd::Refused(CallRefusal::Denied(_)) | StepEnd::Held(_)
        ),
        "the breaker or the repeat guard stops it: {reply:?}"
    );
    assert_eq!(reply.ending, Ending::Done);
}
