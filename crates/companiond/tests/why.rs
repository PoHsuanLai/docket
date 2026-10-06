//! The answer says which model answered, how it was reached and why: inferd's `Why`, `Stage` and
//! `Declined` events become the answer's route notes (`Answer.Routing`), and a refusal of a named
//! model says which and why.

mod support;

use companion_wire::{
    AnswerBody, AnswerPhase, AnswerWire, AskWire, RefusalWire, RouteNote, WhyWord, footer_line,
};
use companiond::serve_on;
use docket_core::*;
use docket_dbus::{CompanionAnswerProxy, CompanionProxy, Details};
use futures_util::StreamExt;
use porter_core::{AccountId, ModelId};
use porter_infer::{
    Declined, DeclinedBecause, Door, InferEvent, InferRefusal, ModelLabel, ModelRef, ProviderId,
    StageNote, StageRole, Why,
};
use prov::AgentRef;
use std::sync::Arc;
use std::time::Duration;
use support::bus::PrivateBus;
use support::infer::{Say, served, words};
use support::world::*;
use tokio::sync::Mutex;

fn model(id: &str) -> ModelRef {
    ModelRef {
        account: AccountId::parse("local").expect("account"),
        model: ModelId::parse(id).expect("model"),
    }
}

/// What inferd sends when a hosted model is warm and the reasons are shown.
fn hosted() -> Vec<InferEvent> {
    vec![
        InferEvent::Why(Why::Evicted {
            model: model("qwen-3"),
        }),
        InferEvent::Why(Why::Warm),
        InferEvent::Why(Why::Reached {
            provider: ProviderId("openrouter".into()),
            door: Door::Gateway,
        }),
        InferEvent::Routed(served()),
        InferEvent::Stage(StageNote {
            role: StageRole::Answer,
            served: served(),
            why: Why::Warm,
            name: Some(ModelLabel("Scripted One".into())),
        }),
    ]
}

const LINE: &str = "Answered by Scripted One via OpenRouter (unloaded Qwen 3, already loaded)";

#[tokio::test]
async fn the_answer_keeps_the_turns_route_and_the_line_reads_it() {
    let mut w = world(vec![Say::Routed(hosted(), Box::new(words("Done.")))]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    let notes = w.companion.shared.route_of(&opened.task).expect("notes");
    assert_eq!(footer_line(&notes), LINE);
    assert_eq!(notes.len(), 1);
    assert_eq!(
        notes[0].why,
        vec![
            WhyWord::Evicted {
                model: model("qwen-3")
            },
            WhyWord::Warm
        ]
    );
    // The footer's `served` is as before: the route is additive.
    let view = w.companion.shared.answer(&opened.task).expect("answer");
    assert_eq!(view.footer.served, vec![served()]);
}

#[tokio::test]
async fn a_turn_that_announces_nothing_leaves_no_route() {
    let mut w = world(vec![words("Plain.")]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    // The scripted model sends only `Finished`: no stage, no route.
    assert_eq!(w.companion.shared.route_of(&opened.task), Some(vec![]));
}

#[tokio::test]
async fn a_named_model_that_cannot_serve_is_a_refusal_that_says_which_and_why() {
    let declined = Declined {
        model: model("kimi-k2.6"),
        because: DeclinedBecause::NotInstalled,
    };
    let mut w = world(vec![Say::Routed(
        vec![InferEvent::Declined(declined)],
        Box::new(Say::Refuse(InferRefusal::Unavailable)),
    )]);
    let opened = w.open("work").await;
    w.say(&opened.session, "hello").await;
    let view: AnswerWire = w.companion.shared.answer(&opened.task).expect("answer");
    assert_eq!(view.phase, AnswerPhase::Failed);
    assert_eq!(
        view.body,
        AnswerBody::Refused(RefusalWire::Failed(
            "Kimi K2.6 cannot answer: it is not installed on this computer.".into()
        ))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_answer_object_serves_the_routing_property() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, client) = (bus.connect().await, bus.connect().await);
    let w = world(vec![Say::Routed(hosted(), Box::new(words("Done.")))]);
    let (launcher, hand) = (w.launcher, w.hand);
    let companion = Arc::new(Mutex::new(w.companion));
    serve_on(&server, companion.clone()).await.expect("serving");
    client
        .request_name("org.quire.Shell")
        .await
        .expect("the shell's name");
    let proxy = CompanionProxy::new(&client).await.expect("proxy");
    let mut added = proxy.receive_answer_added().await.expect("signal");
    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
    };
    let opened: SessionOpened = serde_json::from_str(
        &proxy
            .open(&serde_json::to_string(&open).expect("json"))
            .await
            .expect("open"),
    )
    .expect("opened");
    let turn = launcher
        .session_turn(
            opened.session.clone(),
            TurnIn {
                text: "hi".into(),
                origin: Origin::Launcher,
                keep: keep_nothing(),
                via: TurnVia::Typed,
            },
        )
        .await
        .expect("turn");
    let ask = AskWire {
        session: opened.session.clone(),
        turn: UserTurn {
            id: turn,
            text: "hi".into(),
            at: prov::UnixSeconds(hand.load(std::sync::atomic::Ordering::SeqCst)),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        },
        keep: keep_nothing(),
        parent_window: WindowKey::parse("w1").expect("window"),
        app: None,
    };
    let path = proxy
        .ask(&serde_json::to_string(&ask).expect("json"), &Details::new())
        .await
        .expect("ask");
    tokio::time::timeout(Duration::from_secs(2), added.next())
        .await
        .expect("AnswerAdded in time")
        .expect("signal");
    let answers = CompanionAnswerProxy::builder(&client)
        .path(path)
        .expect("path")
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .expect("answer proxy");
    for _ in 0..100 {
        let view: AnswerWire =
            serde_json::from_str(&answers.view().await.expect("view")).expect("answer");
        if matches!(view.phase, AnswerPhase::Done) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let notes: Vec<RouteNote> =
        serde_json::from_str(&answers.routing().await.expect("routing")).expect("notes");
    assert_eq!(footer_line(&notes), LINE);
}
