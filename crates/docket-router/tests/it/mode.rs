//! The mode of a conversation: Auto or Manual, switched by the shell from the next step, followed
//! by the sessions that work for it, held by the administrator's floor, and refused to everyone
//! the person is not.

use crate::support::*;
use cua_action::{CuaAction, WindowSpace};
use docket_core::*;
use docket_fake::ScriptedConfirmer;
use prov::{AgentRef, ConfirmReceipt, Effect, InputProof, Label, RunId, SessionId, UnixSeconds};

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        written: None,
        scope: GrantScope::Once,
        receipt: ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

fn archive(thread: &str) -> CallRequest {
    call("mail.thread.archive", &[thread], vec![])
}

fn asked(end: &Result<Outcome, CallRefusal>) -> bool {
    matches!(end, Err(CallRefusal::Unconfirmed(_)))
}

/// The person's words in `session`, naming `mode` when given.
async fn say_in(router: &Router, session: &SessionId, mode: Option<Mode>) {
    let reply = ask(
        router,
        &launcher(),
        IntentsRequest::SessionTurn {
            session: session.clone(),
            turn: TurnIn {
                text: "tidy my inbox".into(),
                origin: Origin::Launcher,
                keep: ContextKeep {
                    query: Keep::Dropped,
                    results: Keep::Dropped,
                    selection: Keep::Dropped,
                    window: Keep::Dropped,
                },
                via: TurnVia::Typed,
                mode,
            },
        },
    )
    .await;
    assert!(matches!(reply, IntentsReply::TurnRecorded(_)), "{reply:?}");
}

/// A conversation ready to act, as `ready` makes one, whose first turn names `mode`.
async fn ready_in(router: &Router, mode: Option<Mode>) -> SessionOpened {
    let opened = open(router, "work", AgentRef::Companion).await;
    say_in(router, &opened.session, mode).await;
    give_policy(router, &opened.session, wide_policy(&opened.task, "work"));
    grant_mail(router, "work");
    opened
}

async fn switch(router: &Router, who: &CallerId, session: &SessionId, mode: Mode) -> IntentsReply {
    ask(
        router,
        who,
        IntentsRequest::SessionMode(SessionModeAsk {
            session: session.clone(),
            mode,
        }),
    )
    .await
}

/// A call made in a named session.
async fn perform_in(
    router: &Router,
    session: &SessionId,
    request: CallRequest,
) -> Result<Outcome, CallRefusal> {
    let reply = ask(
        router,
        &companion(),
        IntentsRequest::Perform {
            activation: None,
            call: request,
            session: Some(session.clone()),
            parent_window: None,
        },
    )
    .await;
    match reply {
        IntentsReply::Performed(result) => *result,
        other => panic!("perform: {other:?}"),
    }
}

type Router = docket_router::Router<docket_fake::FakeSeams>;

#[tokio::test]
async fn a_turn_that_names_manual_makes_the_next_change_ask() {
    let router = router();
    let auto = ready_in(&router, None).await;
    assert_eq!(
        auto.mode,
        Mode::Auto,
        "a new conversation starts in the start mode"
    );
    perform_in(&router, &auto.session, archive("t1"))
        .await
        .expect("Auto: an archive inside the request runs");
    assert!(router.seams.confirmer.requests().is_empty());

    let manual = ready_in(&router, Some(Mode::Manual)).await;
    let end = perform_in(&router, &manual.session, archive("t2")).await;
    assert!(asked(&end), "{end:?}");
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    assert!(!router.seams.link.mail.is_archived("t2"));
}

#[tokio::test]
async fn a_turn_from_anyone_but_the_shell_cannot_name_a_mode() {
    let router = router();
    let editor = caller("org.example.Editor", CallerRole::Editor);
    let opened = open_as(&router, &editor, "work", AgentRef::Companion).await;
    let turn = |mode| IntentsRequest::SessionTurn {
        session: opened.session.clone(),
        turn: TurnIn {
            text: "go".into(),
            origin: Origin::Launcher,
            keep: ContextKeep {
                query: Keep::Dropped,
                results: Keep::Dropped,
                selection: Keep::Dropped,
                window: Keep::Dropped,
            },
            via: TurnVia::Typed,
            mode: Some(mode),
        },
    };
    let mode_now = || {
        let st = router.state.lock().expect("lock");
        st.sessions.get(&opened.session).expect("session").mode
    };
    let reply = ask(&router, &editor, turn(Mode::Manual)).await;
    assert!(matches!(reply, IntentsReply::TurnRecorded(_)), "{reply:?}");
    assert_eq!(mode_now(), Mode::Auto, "an editor's turn changes nothing");
    let reply = ask(&router, &launcher(), turn(Mode::Manual)).await;
    assert!(matches!(reply, IntentsReply::TurnRecorded(_)), "{reply:?}");
    assert_eq!(mode_now(), Mode::Manual, "the shell's turn does");
}

#[tokio::test]
async fn the_shell_switches_the_mode_and_no_other_role_can() {
    let router = router();
    let front = ready_in(&router, None).await;
    assert_eq!(
        switch(&router, &launcher(), &front.session, Mode::Manual).await,
        IntentsReply::Done
    );
    let end = perform_in(&router, &front.session, archive("t1")).await;
    assert!(asked(&end), "{end:?}");
    for (name, role) in [
        ("org.quire.Companiond", CallerRole::Companion),
        ("org.quire.AcpAgent", CallerRole::AcpAgent),
        ("org.quire.Cuad", CallerRole::Cua),
        ("org.quire.Do", CallerRole::Cli),
        ("org.quire.Actions", CallerRole::Mcp),
        ("org.quire.Mail", CallerRole::App),
        ("org.quire.Notes", CallerRole::Field),
        ("org.example.Editor", CallerRole::Editor),
    ] {
        let reply = switch(&router, &caller(name, role), &front.session, Mode::Auto).await;
        assert_eq!(
            reply,
            IntentsReply::Refused(WireRefusal::NotAllowed),
            "{role:?}"
        );
    }
    let end = perform_in(&router, &front.session, archive("t2")).await;
    assert!(asked(&end), "nobody but the shell moved it: {end:?}");
    // The control centre switches too, and an unknown session is unknown.
    assert_eq!(
        switch(&router, &control(), &front.session, Mode::Auto).await,
        IntentsReply::Done
    );
    let gone = SessionId::parse("s-9999").expect("session");
    assert_eq!(
        switch(&router, &launcher(), &gone, Mode::Auto).await,
        IntentsReply::Refused(WireRefusal::NoSuchSession)
    );
}

#[tokio::test]
async fn a_sheet_that_is_waiting_keeps_its_answer_when_the_mode_switches() {
    let mut router = router();
    let front = ready_in(&router, Some(Mode::Manual)).await;
    router.seams.confirmer = ScriptedConfirmer::yielding(vec![yes()]);
    let shell = launcher();
    let (end, switched) = tokio::join!(
        perform_in(&router, &front.session, archive("t1")),
        switch(&router, &shell, &front.session, Mode::Auto),
    );
    assert_eq!(switched, IntentsReply::Done);
    end.expect("the sheet was shown under Manual and its yes stands");
    assert_eq!(router.seams.confirmer.requests().len(), 1);
    // From the next step it is Auto: another archive runs without a sheet.
    perform_in(&router, &front.session, archive("t2"))
        .await
        .expect("Auto from the next step");
    assert_eq!(router.seams.confirmer.requests().len(), 1, "no new sheet");
}

#[tokio::test]
async fn a_child_takes_its_parents_mode_and_follows_its_changes() {
    let router = router();
    let front = ready_in(&router, Some(Mode::Manual)).await;
    let reply = ask(
        &router,
        &companion(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Companion,
            parent: Some(front.task.clone()),
            cwd: None,
            started_from: None,
            external: None,
            helper: None,
            acts_for: None,
        }),
    )
    .await;
    let IntentsReply::SessionOpened(child) = reply else {
        panic!("{reply:?}");
    };
    assert_eq!(child.mode, Mode::Manual, "it opens in its parent's mode");
    say_in(&router, &child.session, None).await;
    grant_mail(&router, "work");
    let end = perform_in(&router, &child.session, archive("t1")).await;
    assert!(asked(&end), "{end:?}");
    // The parent moves to Auto; the child follows from its next step.
    assert_eq!(
        switch(&router, &launcher(), &front.session, Mode::Auto).await,
        IntentsReply::Done
    );
    perform_in(&router, &child.session, archive("t2"))
        .await
        .expect("the child follows its parent to Auto");
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

async fn open_run(router: &Router, run: &RunId, parent: Option<prov::TaskId>) {
    let reply = ask(
        router,
        &companion(),
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Cua { run: run.clone() },
            parent,
            cwd: None,
            started_from: None,
            external: None,
            helper: None,
            acts_for: None,
        }),
    )
    .await;
    assert!(matches!(reply, IntentsReply::SessionOpened(_)), "{reply:?}");
}

/// Opens a run's session as `who`, naming the conversation it works for.
async fn open_run_for(router: &Router, who: &CallerId, run: &RunId, conversation: &SessionId) {
    let reply = ask(
        router,
        who,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Cua { run: run.clone() },
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
            helper: None,
            acts_for: Some(conversation.clone()),
        }),
    )
    .await;
    assert!(matches!(reply, IntentsReply::SessionOpened(_)), "{reply:?}");
}

async fn gate(router: &Router, run: &RunId, effect: Effect) -> IntentsReply {
    ask(
        router,
        &caller("org.quire.Cuad", CallerRole::Cua),
        IntentsRequest::GateCheck(step(run, effect)),
    )
    .await
}

#[tokio::test]
async fn a_screen_run_takes_the_mode_of_the_conversation_that_started_it() {
    let mut router = router();
    let front = ready_in(&router, Some(Mode::Manual)).await;
    let linked = RunId::parse("r-1").expect("run");
    let unlinked = RunId::parse("r-2").expect("run");
    open_run(&router, &linked, Some(front.task.clone())).await;
    open_run(&router, &unlinked, None).await;
    // The person allows the app for both runs, once.
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let granted = ask(
        &router,
        &caller("org.quire.Cuad", CallerRole::Cua),
        IntentsRequest::GateGrant(GrantAsk {
            app: mail_app(),
            space: space("work"),
            level: Default::default(),
            run: None,
        }),
    )
    .await;
    assert_eq!(granted, IntentsReply::Granted(GrantAnswer::Granted));
    router.seams.confirmer = ScriptedConfirmer::answering(vec![]);
    assert_eq!(
        gate(&router, &unlinked, Effect::UndoableWrite).await,
        IntentsReply::Gate(GateAnswer::Run),
        "a run no conversation started is in the start mode, Auto, which asks once"
    );
    let linked_step = gate(&router, &linked, Effect::UndoableWrite).await;
    assert!(
        matches!(
            linked_step,
            IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(_)))
        ),
        "Manual asks at each change: {linked_step:?}"
    );
    // The conversation moves to Auto; its run follows from the next step.
    switch(&router, &launcher(), &front.session, Mode::Auto).await;
    assert_eq!(
        gate(&router, &linked, Effect::UndoableWrite).await,
        IntentsReply::Gate(GateAnswer::Run)
    );
}

#[tokio::test]
async fn the_start_mode_and_the_rows_of_the_settings_rule_a_conversation() {
    let router = router_with(AgentConfig {
        start_mode: Mode::Manual,
        ..AgentConfig::default()
    });
    let front = ready_in(&router, None).await;
    assert_eq!(front.mode, Mode::Manual);
    let end = perform_in(&router, &front.session, archive("t1")).await;
    assert!(asked(&end), "{end:?}");
    // Manual's "inside" row set to Goes ahead applies from the next step.
    let mut config = AgentConfig {
        start_mode: Mode::Manual,
        ..AgentConfig::default()
    };
    config.rows.manual.inside = InsideRow::Runs;
    router.apply_settings(config);
    perform_in(&router, &front.session, archive("t2"))
        .await
        .expect("the row now lets an archive inside the request run");
}

#[tokio::test]
async fn a_floor_that_holds_manual_locks_every_conversation_and_the_switch() {
    let mut router = router();
    router.floor = Floor::from_file(FloorFile {
        strictness: Some(Strictness::AskMore),
        ..FloorFile::default()
    })
    .expect("floor");
    let front = ready_in(&router, Some(Mode::Auto)).await;
    assert_eq!(front.mode, Mode::Manual, "the floor holds it");
    assert_eq!(
        switch(&router, &launcher(), &front.session, Mode::Auto).await,
        IntentsReply::Refused(WireRefusal::Locked)
    );
    let end = perform_in(&router, &front.session, archive("t1")).await;
    assert!(asked(&end), "even a turn that names Auto: {end:?}");
}

#[tokio::test]
async fn a_row_the_floor_locks_asks_in_both_modes() {
    let mut router = router();
    router.floor = Floor::from_file(FloorFile {
        rows: vec![LockedRow::Inside, LockedRow::Other],
        ..FloorFile::default()
    })
    .expect("floor");
    let front = ready_in(&router, None).await;
    let end = perform_in(&router, &front.session, archive("t1")).await;
    assert!(asked(&end), "Auto's inside row is locked to asks: {end:?}");
}

#[tokio::test]
async fn a_run_follows_the_conversation_the_screen_daemon_names_and_nobody_else_may_name_one() {
    let mut router = router();
    let front = ready_in(&router, Some(Mode::Manual)).await;
    let named = RunId::parse("r-1").expect("run");
    let borrowed = RunId::parse("r-2").expect("run");
    let cuad = caller("org.quire.Cuad", CallerRole::Cua);
    open_run_for(&router, &cuad, &named, &front.session).await;
    // The companion may open a run's session, but its word on the conversation is ignored.
    open_run_for(&router, &companion(), &borrowed, &front.session).await;
    router.seams.confirmer = ScriptedConfirmer::answering(vec![yes()]);
    let granted = ask(
        &router,
        &cuad,
        IntentsRequest::GateGrant(GrantAsk {
            app: mail_app(),
            space: space("work"),
            level: Default::default(),
            run: None,
        }),
    )
    .await;
    assert_eq!(granted, IntentsReply::Granted(GrantAnswer::Granted));
    router.seams.confirmer = ScriptedConfirmer::answering(vec![]);
    let step = gate(&router, &named, Effect::UndoableWrite).await;
    assert!(
        matches!(
            step,
            IntentsReply::Gate(GateAnswer::Refused(CallRefusal::Unconfirmed(_)))
        ),
        "the run works for a Manual conversation, so each change asks: {step:?}"
    );
    assert_eq!(
        gate(&router, &borrowed, Effect::UndoableWrite).await,
        IntentsReply::Gate(GateAnswer::Run),
        "a conversation named by anyone but the screen daemon is ignored: the start mode, Auto"
    );
}
