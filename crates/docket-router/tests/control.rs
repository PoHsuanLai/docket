//! The halt, the computer-use gate and callers that are not the person's own surfaces.

mod support;

use cua_action::{CuaAction, WindowSpace};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::GrantStore;
use prov::{AgentRef, ConfirmReceipt, Effect, InputProof, Label, RunId, SpaceScope, UnixSeconds};
use support::*;

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
        },
    }
}

fn step(run: &RunId, effect: Effect) -> CuaAsk {
    CuaAsk {
        run: run.clone(),
        step: 1,
        app: mail_app(),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: space("work"),
        action: CuaAction::<WindowSpace>::Observe,
        node: None,
        effect,
        basis: EffectBasis::DefaultTable,
        screen: Label::untrusted(
            prov::Source::Screen { app: mail_app() },
            prov::DataClass::Mail,
            space("work"),
        ),
    }
}

async fn run_session(router: &docket_router::Router<docket_fake::FakeSeams>, run: &RunId) {
    let reply = ask(
        router,
        &companion(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Cua { run: run.clone() },
            parent: None,
        }),
    )
    .await;
    assert!(matches!(reply, IntentsReply::SessionOpened(_)), "{reply:?}");
}

#[tokio::test]
async fn a_pixel_step_needs_the_persons_consent_for_the_app_and_then_a_read_runs() {
    let mut router = router();
    let run = RunId::parse("r-1").expect("run");
    run_session(&router, &run).await;
    let first = ask(
        &router,
        &cuad(),
        IntentsRequest::GateCheck(step(&run, Effect::Read)),
    )
    .await;
    assert_eq!(
        first,
        IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(
            ConfirmEnd::Dismissed
        ))),
        "no consent yet: the person is asked, and said nothing"
    );
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let granted = ask(
        &router,
        &cuad(),
        IntentsRequest::GateGrant(GrantAsk {
            app: mail_app(),
            space: space("work"),
        }),
    )
    .await;
    assert_eq!(granted, IntentsReply::Granted(GrantAnswer::Granted));
    assert!(!router.seams.grants.grants().is_empty());
    let second = ask(
        &router,
        &cuad(),
        IntentsRequest::GateCheck(step(&run, Effect::Read)),
    )
    .await;
    assert_eq!(second, IntentsReply::Gate(GateAnswer::Run));
    let outbound = ask(
        &router,
        &cuad(),
        IntentsRequest::GateCheck(step(&run, Effect::Outbound)),
    )
    .await;
    assert!(
        matches!(
            outbound,
            IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(_)))
        ),
        "a step that sends asks again: {outbound:?}"
    );
}

#[tokio::test]
async fn a_dismissed_grant_request_records_nothing() {
    let router = router();
    let reply = ask(
        &router,
        &cuad(),
        IntentsRequest::GateGrant(GrantAsk {
            app: mail_app(),
            space: space("work"),
        }),
    )
    .await;
    assert_eq!(
        reply,
        IntentsReply::Granted(GrantAnswer::Refused(ConfirmEnd::Dismissed))
    );
    assert!(router.seams.grants.grants().is_empty());
}

#[tokio::test]
async fn a_step_of_a_run_nobody_opened_is_refused() {
    let router = router();
    let run = RunId::parse("r-9").expect("run");
    let reply = ask(
        &router,
        &cuad(),
        IntentsRequest::GateCheck(step(&run, Effect::Read)),
    )
    .await;
    assert!(
        matches!(reply, IntentsReply::Gate(GateAnswer::Refused(_))),
        "{reply:?}"
    );
}

#[tokio::test]
async fn a_halt_withdraws_every_sheet_in_scope_and_only_the_control_centre_resumes() {
    let router = router();
    let open_sheet = |id: &str, in_space: &str| {
        router
            .state
            .lock()
            .expect("lock")
            .pending
            .insert(prov::ConfirmId::parse(id).expect("id"), space(in_space));
    };
    open_sheet("c-1", "work");
    open_sheet("c-2", "home");
    let halted = ask(
        &router,
        &control(),
        IntentsRequest::ControlHalt {
            scope: SpaceScope::Only(space("work")),
            cause: HaltCause::ControlCentre,
        },
    )
    .await;
    assert_eq!(halted, IntentsReply::Done);
    assert_eq!(
        router.seams.confirmer.cancelled(),
        vec![prov::ConfirmId::parse("c-1").expect("id")]
    );
    let IntentsReply::State(kill) = ask(
        &router,
        &launcher_with(CallerRole::Control),
        IntentsRequest::ControlState,
    )
    .await
    else {
        panic!("state")
    };
    assert!(kill.spaces.contains_key(&space("work")) && !kill.spaces.contains_key(&space("home")));
    let denied = ask(
        &router,
        &caller("org.quire.Compositor", CallerRole::Compositor),
        IntentsRequest::ControlResume {
            scope: SpaceScope::Any,
        },
    )
    .await;
    assert_eq!(
        denied,
        IntentsReply::Refused(WireRefusal::NotAllowed),
        "the compositor may halt, not resume"
    );
    ask(
        &router,
        &control(),
        IntentsRequest::ControlResume {
            scope: SpaceScope::Only(space("work")),
        },
    )
    .await;
    let IntentsReply::State(kill) = ask(&router, &control(), IntentsRequest::ControlState).await
    else {
        panic!()
    };
    assert!(kill.spaces.is_empty());
    let records = router.seams.sink.records();
    assert_eq!(
        records
            .iter()
            .filter(|r| matches!(r, AuditRecord::Halt { .. }))
            .count(),
        2,
        "begin and end"
    );
}

fn launcher_with(role: CallerRole) -> CallerId {
    caller("org.quire.Shell", role)
}

#[tokio::test]
async fn an_mcp_clients_arguments_are_untrusted_and_its_writes_ask() {
    let router = router();
    let mcp = caller("claude-desktop.example", CallerRole::Mcp);
    // A client name is not a reverse-DNS app name, so the connection layer gives it one.
    let mcp = CallerId {
        app: porter_core::AppId {
            name: app("org.example.ClaudeDesktop"),
            ..mcp.app
        },
        ..mcp
    };
    let read = ask(
        &router,
        &mcp,
        IntentsRequest::Perform {
            call: call("mail.thread.read", &["t1"], vec![]),
            parent_window: None,
        },
    )
    .await;
    assert!(
        matches!(read, IntentsReply::Performed(ref r) if r.is_err()),
        "no grant yet: {read:?}"
    );
    let archive = ask(
        &router,
        &mcp,
        IntentsRequest::Perform {
            call: call("mail.thread.archive", &["t1"], vec![]),
            parent_window: None,
        },
    )
    .await;
    let IntentsReply::Performed(result) = archive else {
        panic!()
    };
    assert_eq!(
        *result,
        Err(CallRefusal::Unconfirmed(ConfirmEnd::Dismissed))
    );
    assert!(!router.seams.link.mail.is_archived("t1"));
}
