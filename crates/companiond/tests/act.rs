//! `Companion1.Answer.Act` on a private bus: a card of the answer becomes a watched `Run.Perform`
//! in the answer's session, the answer follows it (`Updated`), and only the shell may press.

mod support;

use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, CardWire, StepWireState};
use companiond::serve_on;
use docket_core::*;
use docket_dbus::{CompanionAnswerProxy, CompanionProxy};
use futures_util::StreamExt;
use prov::Effect;
use std::sync::Arc;
use std::time::Duration;
use support::bus::PrivateBus;
use support::world::*;
use tokio::sync::Mutex;

fn card(id: &str, action: &str, key: &str) -> CardWire {
    CardWire {
        id: CardActionId::parse(id).expect("card id"),
        label: LabelText::parse("Archive").expect("label"),
        effect: Effect::UndoableWrite,
        call: CallRequest {
            action: ActionRef {
                app: app("org.quire.Mail"),
                name: prov::ActionName::parse(action).expect("action"),
            },
            target: TargetValue::Entities(vec![entity("mail.thread", key)]),
            args: Args::new(),
            origin: Origin::Companion,
        },
    }
}

fn draft(cards: Vec<CardWire>) -> AnswerBody {
    AnswerBody::DraftReply {
        to: vec![],
        subject: Reveal::Plain("Re: Digest".into()),
        body: Reveal::Plain("Thanks.".into()),
        actions: cards,
    }
}

async fn step_after_done(
    updates: &mut (impl futures_util::Stream<Item = String> + Unpin),
    steps: usize,
) -> AnswerWire {
    let seen = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = updates.next().await.expect("Updated");
            let answer: AnswerWire = serde_json::from_str(&view).expect("answer");
            let held = match &answer.body {
                AnswerBody::Plan(plan) => plan.steps.len(),
                _ => 0,
            };
            if answer.phase == AnswerPhase::Done && held == steps {
                return answer;
            }
        }
    })
    .await;
    seen.expect("the answer reached Done")
}

fn only_step(answer: &AnswerWire) -> StepWireState {
    let AnswerBody::Plan(plan) = &answer.body else {
        panic!("a card that was pressed shows its step: {:?}", answer.body)
    };
    assert_eq!(plan.steps.len(), 1);
    plan.steps[0].state.clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pressed_card_is_performed_in_the_answers_session_and_the_answer_follows_it() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let opened = w.open("work").await;
    let router = w.router.clone();
    let companion = Arc::new(Mutex::new(w.companion));
    serve_on(&server, companion.clone()).await.expect("serving");
    shell.request_name("org.quire.Shell").await.expect("shell");
    let proxy = CompanionProxy::new(&shell).await.expect("proxy");
    let mut added = proxy.receive_answer_added().await.expect("signal");

    // A draft with two cards: one that works, one whose thread is gone.
    companion
        .lock()
        .await
        .propose(
            &opened.task,
            draft(vec![
                card("archive", "mail.thread.archive", "t1"),
                card("gone", "mail.thread.archive", "t404"),
            ]),
        )
        .expect("proposal");
    let path = added
        .next()
        .await
        .expect("AnswerAdded")
        .args()
        .expect("args")
        .answer
        .to_string();
    let answers_of = |connection: &docket_dbus::BusConnection| {
        let connection = connection.clone();
        let path = path.clone();
        async move {
            CompanionAnswerProxy::builder(&connection)
                .path(path)
                .expect("path")
                .cache_properties(zbus::proxy::CacheProperties::No)
                .build()
                .await
                .expect("answer proxy")
        }
    };
    let (answers, intruder) = (answers_of(&shell).await, answers_of(&stranger).await);

    // The answer offers the cards; before anything is pressed it is the draft.
    let first: AnswerWire = serde_json::from_str(&answers.view().await.expect("view")).expect("a");
    assert!(
        matches!(first.body, AnswerBody::DraftReply { .. }),
        "{first:?}"
    );

    // Only the shell may press.
    assert!(intruder.act("archive").await.is_err());
    assert!(router.seams.link.mail.has_thread("t1"));
    assert!(!router.seams.link.mail.is_archived("t1"));
    // A card the answer does not offer is no card.
    assert!(answers.act("nope").await.is_err());

    // Pressing runs the card's own call as the companion, in the answer's session.
    let mut updates = Box::pin(
        answers
            .receive_updated()
            .await
            .expect("updates")
            .map(|signal| signal.args().expect("args").view.to_string()),
    );
    let request = answers.act("archive").await.expect("act");
    assert!(
        request.as_str().starts_with("/org/quire/Intents1/request/"),
        "{request}"
    );
    let done = step_after_done(&mut updates, 1).await;
    let StepWireState::Done { undo } = only_step(&done) else {
        panic!("the thread was archived: {done:?}")
    };
    assert!(undo.is_some(), "the journal's row comes with the step");
    assert!(router.seams.link.mail.is_archived("t1"));

    // A call that fails ends its step Failed; the answer is Done and holds both steps.
    answers.act("gone").await.expect("act");
    let both = step_after_done(&mut updates, 2).await;
    let AnswerBody::Plan(plan) = &both.body else {
        panic!("{:?}", both.body)
    };
    assert!(
        matches!(
            plan.steps[1].state,
            StepWireState::Failed(CallRefusal::App(AppRefusal::NotFound(_)))
        ),
        "{:?}",
        plan.steps[1].state
    );
    assert!(matches!(plan.steps[0].state, StepWireState::Done { .. }));
}
