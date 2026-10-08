//! The computer-use daemon and its run: it opens the session of its own run (and no other kind),
//! and a requester that watches a gate check is told when the person is about to be asked,
//! decides when the sheet may be drawn (`Proceed`) and may take the request back (`Close`).

use crate::support::*;
use cua_action::{CuaAction, WindowSpace};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::{Flag, Queue, Watch};
use prov::{AgentRef, ConfirmReceipt, Effect, InputProof, Label, RunId, UnixSeconds};
use std::sync::Arc;

fn cuad() -> CallerId {
    caller("org.quire.Cuad", CallerRole::Cua)
}

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

fn step(run: &RunId) -> CuaAsk {
    CuaAsk {
        run: run.clone(),
        step: 1,
        app: mail_app(),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: space("work"),
        action: CuaAction::<WindowSpace>::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: Label::untrusted(
            prov::Source::Screen { app: mail_app() },
            prov::DataClass::Mail,
            space("work"),
        ),
    }
}

fn open_run(run: &RunId) -> IntentsRequest {
    IntentsRequest::SessionOpen(SessionOpen {
        space: space("work"),
        agent: AgentRef::Cua { run: run.clone() },
        parent: None,
        cwd: None,
        started_from: None,
        external: None,
    })
}

#[tokio::test]
async fn cuad_opens_the_session_of_its_own_run_and_its_first_step_is_ruled_not_halted() {
    let router = router();
    let run = RunId::parse("r-1").expect("run");
    let opened = ask(&router, &cuad(), open_run(&run)).await;
    assert!(
        matches!(opened, IntentsReply::SessionOpened(_)),
        "{opened:?}"
    );
    let first = ask(&router, &cuad(), IntentsRequest::GateCheck(step(&run))).await;
    assert_eq!(
        first,
        IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(
            ConfirmEnd::Dismissed
        ))),
        "the run has a session: the step reaches the person, it is not a halt"
    );
}

#[tokio::test]
async fn cuad_opens_a_run_and_nothing_else_and_a_run_has_one_session() {
    let router = router();
    let run = RunId::parse("r-2").expect("run");
    for agent in [AgentRef::Companion, AgentRef::User] {
        let reply = ask(
            &router,
            &cuad(),
            IntentsRequest::SessionOpen(SessionOpen {
                space: space("work"),
                agent,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            }),
        )
        .await;
        assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
    }
    assert!(matches!(
        ask(&router, &cuad(), open_run(&run)).await,
        IntentsReply::SessionOpened(_)
    ));
    assert_eq!(
        ask(&router, &cuad(), open_run(&run)).await,
        IntentsReply::Refused(WireRefusal::Malformed),
        "a second session for the same run"
    );
    assert_eq!(
        ask(&router, &companion(), open_run(&run)).await,
        IntentsReply::Refused(WireRefusal::Malformed),
        "whoever asks"
    );
}

#[tokio::test]
async fn cuad_closes_only_the_sessions_it_opened() {
    let router = router();
    let run = RunId::parse("r-3").expect("run");
    let IntentsReply::SessionOpened(mine) = ask(&router, &cuad(), open_run(&run)).await else {
        panic!("opened")
    };
    let theirs = open(&router, "work", AgentRef::Companion).await;
    let close = |session| IntentsRequest::SessionClose { session };
    assert_eq!(
        ask(&router, &cuad(), close(theirs.session)).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(
        ask(&router, &cuad(), close(mine.session)).await,
        IntentsReply::Done
    );
}

/// The requester's end of a watched request.
struct Remote {
    progress: Arc<Queue<CallProgress>>,
    proceed: Arc<Flag>,
    closed: Arc<Flag>,
}

fn watched() -> (Watch, Remote) {
    let remote = Remote {
        progress: Queue::new(),
        proceed: Flag::new(),
        closed: Flag::new(),
    };
    let (progress, proceed, closed) = (
        remote.progress.clone(),
        remote.proceed.clone(),
        remote.closed.clone(),
    );
    let watch = Watch::new(
        move |p| {
            progress.push(p);
            Box::pin(std::future::ready(()))
        },
        move || Box::pin(proceed.up()),
        move || Box::pin(closed.up()),
    );
    (watch, remote)
}

async fn run_with_session(router: &docket_router::Router<docket_fake::FakeSeams>) -> RunId {
    let run = RunId::parse("r-4").expect("run");
    assert!(matches!(
        ask(router, &cuad(), open_run(&run)).await,
        IntentsReply::SessionOpened(_)
    ));
    run
}

#[tokio::test]
async fn a_watcher_is_told_before_the_sheet_and_the_sheet_waits_for_proceed() {
    let mut router = router();
    let run = run_with_session(&router).await;
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let (watch, remote) = watched();
    let caller = cuad();
    let check = router.handle_watched(&caller, IntentsRequest::GateCheck(step(&run)), watch);
    let remote_side = async {
        let CallProgress::Confirming(id) = remote.progress.next().await else {
            panic!("the first thing said is that the person will be asked")
        };
        // Whatever the requester does now (suspending its own input), no sheet is drawn.
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert!(router.seams.confirmer.requests().is_empty());
        remote.proceed.raise();
        id
    };
    let (answer, id) = tokio::join!(check, remote_side);
    assert_eq!(answer, IntentsReply::Gate(GateAnswer::Run));
    let shown = router.seams.confirmer.requests();
    assert_eq!(shown.len(), 1);
    assert_eq!(
        shown[0].id, id,
        "the id the watcher was told is the sheet's"
    );
}

#[tokio::test]
async fn a_request_withdrawn_before_proceed_draws_no_sheet() {
    let mut router = router();
    let run = run_with_session(&router).await;
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let (watch, remote) = watched();
    let caller = cuad();
    let check = router.handle_watched(&caller, IntentsRequest::GateCheck(step(&run)), watch);
    let remote_side = async {
        let _ = remote.progress.next().await;
        remote.closed.raise();
    };
    let (answer, ()) = tokio::join!(check, remote_side);
    assert_eq!(
        answer,
        IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(
            ConfirmEnd::Cancelled
        )))
    );
    assert!(router.seams.confirmer.requests().is_empty());
}

#[tokio::test]
async fn nobody_watching_means_no_wait() {
    let mut router = router();
    let run = run_with_session(&router).await;
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let answer = ask(&router, &cuad(), IntentsRequest::GateCheck(step(&run))).await;
    assert_eq!(answer, IntentsReply::Gate(GateAnswer::Run));
}

fn send_to_a_stranger() -> CallRequest {
    CallRequest {
        action: action("mail.message.send"),
        target: TargetValue::Nothing,
        args: [
            (
                param("to"),
                prov::Labelled {
                    value: Value::Entity(entity("mail.contact", "eve@evil.test")),
                    label: trusted(),
                },
            ),
            (
                param("body"),
                prov::Labelled {
                    value: Value::Text("tidy".into()),
                    label: trusted(),
                },
            ),
        ]
        .into_iter()
        .collect(),
        origin: Origin::Companion,
    }
}

fn performing(call: CallRequest) -> IntentsRequest {
    IntentsRequest::Perform {
        activation: None,
        call,
        session: None,
        parent_window: None,
    }
}

fn said(remote: &Remote) -> Vec<CallProgress> {
    std::iter::from_fn(|| remote.progress.take()).collect()
}

#[tokio::test]
async fn a_perform_tells_a_watcher_how_far_it_is_and_waits_for_nothing() {
    let router = router();
    ready(&router).await;
    let (watch, remote) = watched();
    let caller = companion();
    // An outbound send to a stranger: previewed, then the person is asked (and dismisses).
    let answer = router
        .handle_watched(&caller, performing(send_to_a_stranger()), watch)
        .await;
    assert!(matches!(answer, IntentsReply::Performed(_)), "{answer:?}");
    let progress = said(&remote);
    let shown = router.seams.confirmer.requests();
    assert_eq!(shown.len(), 1);
    assert_eq!(
        progress,
        [
            CallProgress::Previewing,
            CallProgress::Confirming(shown[0].id.clone())
        ],
        "the watcher knows of the sheet as it is drawn, and no Proceed was needed"
    );
}

#[tokio::test]
async fn a_judged_call_says_reviewing_and_then_dispatched() {
    // Under TrustMore an outbound act with every sink trusted is judged by every stage, then runs.
    let mut router = router();
    router.config.strictness = Strictness::TrustMore;
    ready(&router).await;
    let suggested = ask(
        &router,
        &companion(),
        IntentsRequest::Suggest(SuggestAsk {
            action: action("mail.message.send"),
            param: param("to"),
            typed: String::new(),
        }),
    )
    .await;
    assert!(
        matches!(suggested, IntentsReply::Suggestions(_)),
        "{suggested:?}"
    );
    let mut send = send_to_a_stranger();
    send.args.insert(
        param("to"),
        prov::Labelled {
            value: Value::Entity(entity("mail.contact", "c1")),
            label: trusted(),
        },
    );
    let (watch, remote) = watched();
    let answer = router
        .handle_watched(&companion(), performing(send), watch)
        .await;
    assert!(matches!(answer, IntentsReply::Performed(_)), "{answer:?}");
    let progress = said(&remote);
    assert!(progress.contains(&CallProgress::Reviewing), "{progress:?}");
    assert_eq!(
        progress.last(),
        Some(&CallProgress::Dispatched),
        "{progress:?}"
    );
}

#[tokio::test]
async fn an_unwatched_perform_is_as_before() {
    let router = router();
    ready(&router).await;
    let answer = ask(&router, &companion(), performing(send_to_a_stranger())).await;
    assert!(matches!(answer, IntentsReply::Performed(_)), "{answer:?}");
}

#[tokio::test]
async fn a_watcher_that_never_proceeds_is_refused_unasked_after_the_configured_time() {
    use docket_core::{AgentConfig, Millis};
    // The time is a setting, and the clock is virtual: nothing here waits.
    let config = AgentConfig {
        confirm_proceed: Millis(2_500),
        ..AgentConfig::default()
    };
    let mut router = docket_fake::fake_router(config).expect("router");
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let run = run_with_session(&router).await;
    let (watch, _remote) = watched();
    let caller = cuad();
    let check = router.handle_watched(&caller, IntentsRequest::GateCheck(step(&run)), watch);
    let clock_side = async {
        // The router asks the clock for the Proceed deadline: the configured one.
        assert_eq!(router.seams.clock.next_ask().await, Millis(2_500));
        router.seams.clock.advance(Millis(2_499));
        for _ in 0..5 {
            tokio::task::yield_now().await;
        }
        assert_eq!(router.seams.clock.asked(), [Millis(2_500)]);
        router.seams.clock.advance(Millis(1));
    };
    let (answer, ()) = tokio::join!(check, clock_side);
    assert_eq!(
        answer,
        IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(
            ConfirmEnd::Expired
        )))
    );
    assert!(
        router.seams.confirmer.requests().is_empty(),
        "no sheet was drawn"
    );
}

#[tokio::test]
async fn the_proceed_time_is_a_setting_with_a_default_of_ten_seconds() {
    assert_eq!(AgentConfig::default().confirm_proceed, Millis(10_000));
    assert_eq!(
        AgentConfig::default().value("agent.confirm.proceed_ms"),
        Some(SettingValue::Number(10_000))
    );
}
