//! The halt, the computer-use gate and callers that are not the person's own surfaces.

use crate::support::*;
use cua_action::{CuaAction, WindowSpace};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use docket_router::GrantStore;
use prov::{AgentRef, ConfirmReceipt, Effect, InputProof, Label, RunId, SpaceScope, UnixSeconds};

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

#[tokio::test]
async fn resuming_one_space_lifts_a_global_halt_for_that_space_alone() {
    let router = router();
    open(&router, "work", AgentRef::Companion).await;
    open(&router, "home", AgentRef::Companion).await;
    ask(
        &router,
        &control(),
        IntentsRequest::ControlHalt {
            scope: SpaceScope::Any,
            cause: HaltCause::ControlCentre,
        },
    )
    .await;
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
        panic!("state")
    };
    assert_eq!(
        kill.all,
        docket_core::Halt::Running,
        "no global halt is left"
    );
    assert!(
        kill.spaces.contains_key(&space("home")) && !kill.spaces.contains_key(&space("work")),
        "the other Space is still halted: {kill:?}"
    );
    assert_eq!(docket_core::halted(&kill, &space("work")), None);
    assert!(docket_core::halted(&kill, &space("home")).is_some());
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
            activation: None,
            call: call("mail.thread.read", &["t1"], vec![]),
            session: None,
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
            activation: None,
            call: call("mail.thread.archive", &["t1"], vec![]),
            session: None,
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

#[tokio::test]
async fn a_run_that_looked_at_a_screen_reports_with_the_screens_label() {
    let mut router = router();
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let run = RunId::parse("r-1").expect("run");
    let front = open(&router, "work", AgentRef::Companion).await;
    let run_opened = ask(
        &router,
        &companion(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Cua { run: run.clone() },
            parent: Some(front.task.clone()),
        }),
    )
    .await;
    let IntentsReply::SessionOpened(run_opened) = run_opened else {
        panic!("{run_opened:?}");
    };
    let before = report(&router, &run_opened.session).await;
    assert!(matches!(before, IntentsReply::Delivered(_)), "{before:?}");
    let sent = |router: &docket_router::Router<docket_fake::FakeSeams>| {
        router
            .seams
            .sink
            .records()
            .into_iter()
            .filter_map(|r| match r {
                AuditRecord::Message(m) => Some(m.label),
                _ => None,
            })
            .next_back()
            .expect("a message")
    };
    let screen = |label: &Label| {
        label
            .sources
            .contains(&prov::Source::Screen { app: mail_app() })
    };
    assert!(!screen(&sent(&router)), "nothing was on screen yet");
    ask(
        &router,
        &cuad(),
        IntentsRequest::GateGrant(GrantAsk {
            app: mail_app(),
            space: space("work"),
        }),
    )
    .await;
    let step = ask(
        &router,
        &cuad(),
        IntentsRequest::GateCheck(step(&run, Effect::Read)),
    )
    .await;
    assert_eq!(step, IntentsReply::Gate(GateAnswer::Run));
    let after = report(&router, &run_opened.session).await;
    assert!(matches!(after, IntentsReply::Delivered(_)), "{after:?}");
    let label = sent(&router);
    assert!(
        screen(&label),
        "the report carries Untrusted(Screen): {label:?}"
    );
    assert_eq!(label.integrity, prov::Integrity::Untrusted);
}

async fn report(
    router: &docket_router::Router<docket_fake::FakeSeams>,
    session: &prov::SessionId,
) -> IntentsReply {
    let draft = MessageDraft {
        to: prov::Address::new(AgentRef::Companion, space("work")),
        thread: None,
        in_reply_to: None,
        kind: prov::MessageKind::Report {
            status: prov::ReportStatus::Done,
        },
        parts: vec![DraftPart::Text(prov::MessageText::new("opened the page"))],
    };
    ask(
        router,
        &cuad(),
        IntentsRequest::MessageSend {
            session: session.clone(),
            draft,
        },
    )
    .await
}

#[tokio::test]
async fn context_looks_in_the_window_of_the_app_the_caller_names() {
    let router = router();
    let unknown = ask(
        &router,
        &companion(),
        IntentsRequest::Context {
            session: prov::SessionId::parse("s-999").expect("id"),
            app: mail_app(),
        },
    )
    .await;
    assert_eq!(unknown, IntentsReply::Refused(WireRefusal::NoSuchSession));
    let front = open(&router, "work", AgentRef::Companion).await;
    let named = ask(
        &router,
        &companion(),
        IntentsRequest::Context {
            session: front.session,
            app: mail_app(),
        },
    )
    .await;
    // The fakes have no windows, so the app is asked and cannot answer; what matters is that
    // a launcher turn no longer has to name a window first.
    assert!(
        matches!(named, IntentsReply::Refused(WireRefusal::Call(_))),
        "{named:?}"
    );
}
