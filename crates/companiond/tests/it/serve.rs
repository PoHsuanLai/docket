//! `org.quire.Companion1` on a private bus: served as declared, an ask answered at once with the
//! answer's object path while the loop runs behind it, the roster and the front task read without
//! waiting for the loop, and the answer object's view.

use crate::support::bus::PrivateBus;
use crate::support::infer::{Say, words};
use crate::support::world::*;
use companion_wire::{
    AnswerPhase, AnswerWire, AskWire, BroughtBack, FrontTask, HelperForm, HelperReply, HelpersView,
    RoutineCardView, RoutineForm, RoutineProposal, RoutineReply, RoutinesView, TerminalSession,
};
use companiond::{RoutineDesk, serve_on, serve_with_routines};
use docket_core::helper::{Ceiling, HelperModel, HelperName, Role};
use docket_core::routine::RoutineId;
use docket_core::*;
use docket_dbus::{
    Bus, COMPANION_BUS, COMPANION_PATH, CompanionAnswerProxy, CompanionHelpers2Proxy,
    CompanionHelpersProxy, CompanionProxy, CompanionTerminalsProxy, Details, introspection,
};
use docket_tasks::{HelperDesk, Pending, RoutineAsk, RoutineFor};
use futures_util::StreamExt;
use futures_util::lock::Mutex;
use prov::AgentRef;
use std::sync::Arc;
use std::time::Duration;

/// The text of one `<interface name="...">` element.
fn block(xml: &str, name: &str) -> String {
    let start = xml
        .find(&format!("<interface name=\"{name}\">"))
        .unwrap_or_else(|| panic!("{name} missing"));
    let rest = &xml[start..];
    let end = rest.find("</interface>").expect("end") + "</interface>".len();
    rest[..end]
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join("\n")
}

async fn introspect(connection: &docket_dbus::BusConnection, path: &str) -> String {
    zbus::fdo::IntrospectableProxy::builder(connection)
        .destination(COMPANION_BUS)
        .expect("destination")
        .path(path.to_owned())
        .expect("path")
        .build()
        .await
        .expect("proxy")
        .introspect()
        .await
        .expect("introspection")
}

async fn view_until_done(answers: &CompanionAnswerProxy<'_>) -> AnswerWire {
    for _ in 0..100 {
        let view = answers.view().await.expect("view");
        let answer: AnswerWire = serde_json::from_str(&view).expect("answer");
        if matches!(answer.phase, AnswerPhase::Done | AnswerPhase::Failed) {
            return answer;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the answer never finished: {:?}", answers.view().await);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn companion1_is_served_as_declared_and_an_ask_runs_to_its_answer() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, client) = (bus.connect().await, bus.connect().await);
    let w = world(vec![words("Hello there.")]);
    let (launcher, hand) = (w.launcher, w.hand);
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_on(&server, companion.clone(), who.clone())
        .await
        .expect("serving");
    // The client is the shell: only it speaks for the person.
    as_shell(&who, &client);

    let proxy = CompanionProxy::new(&client).await.expect("proxy");
    let mut added = proxy.receive_answer_added().await.expect("signal");

    // Open a front conversation over the bus.
    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
        external: None,
        helper: None,
    };
    let opened: SessionOpened = serde_json::from_str(
        &proxy
            .open(&serde_json::to_string(&open).expect("json"))
            .await
            .expect("open"),
    )
    .expect("opened");
    let front: FrontTask =
        serde_json::from_str(&proxy.front().await.expect("front")).expect("front");
    assert_eq!(front.task, Some(opened.task.clone()));
    assert_eq!(front.session, Some(opened.session.clone()));

    // The person's turn: the router records it; the UI hands it over; the companion is asked.
    let turn = launcher
        .session_turn(
            opened.session.clone(),
            TurnIn {
                text: "hello".into(),
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
            text: "hello".into(),
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
    assert!(
        path.as_str().starts_with("/org/quire/Companion1/answer/"),
        "{path}"
    );
    let signalled = tokio::time::timeout(Duration::from_secs(2), added.next())
        .await
        .expect("AnswerAdded in time")
        .expect("signal");
    assert_eq!(
        signalled.args().expect("args").answer.as_str(),
        path.as_str()
    );

    // The answer object exists at the path the ask returned, and finishes.
    let answers = CompanionAnswerProxy::builder(&client)
        .path(path.clone())
        .expect("path")
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .expect("answer proxy");
    let answer = view_until_done(&answers).await;
    assert_eq!(answer.phase, AnswerPhase::Done);
    assert_eq!(answer.task, opened.task);
    // The task is over: a follow-up on its session is refused on the bus.
    assert!(
        proxy
            .ask(&serde_json::to_string(&ask).expect("json"), &Details::new())
            .await
            .is_err()
    );
    // There is no card to act on.
    assert!(answers.act("{}").await.is_err());

    // Served as declared: the root object and the answer object.
    let declared = introspection(Bus::Companion);
    assert_eq!(
        block(
            &introspect(&client, COMPANION_PATH).await,
            "org.quire.Companion1"
        ),
        block(&declared, "org.quire.Companion1")
    );
    assert_eq!(
        block(
            &introspect(&client, path.as_str()).await,
            "org.quire.Companion1.Answer"
        ),
        block(&declared, "org.quire.Companion1.Answer")
    );

    // Closing the session removes the answer object.
    proxy.close(opened.session.as_str()).await.expect("close");
    assert!(answers.view().await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_roster_and_the_front_answer_while_a_planner_turn_is_still_running() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, client) = (bus.connect().await, bus.connect().await);
    let w = world(vec![Say::Hang]);
    let launcher = w.launcher;
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_on(&server, companion.clone(), who.clone())
        .await
        .expect("serving");
    // The client is the shell: only it speaks for the person.
    as_shell(&who, &client);
    let proxy = CompanionProxy::new(&client).await.expect("proxy");

    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
        external: None,
        helper: None,
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
                text: "hello".into(),
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
            text: "hello".into(),
            at: prov::UnixSeconds(0),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        },
        keep: keep_nothing(),
        parent_window: WindowKey::parse("w1").expect("window"),
        app: None,
    };
    // The model never answers; the call still returns the path, and the bus still reads.
    proxy
        .ask(&serde_json::to_string(&ask).expect("json"), &Details::new())
        .await
        .expect("ask");
    let roster: Roster = tokio::time::timeout(Duration::from_secs(2), proxy.roster())
        .await
        .expect("Roster() does not wait for the loop")
        .map(|s| serde_json::from_str(&s).expect("roster"))
        .expect("roster");
    assert_eq!(roster.entries.len(), 1);
    assert_eq!(roster.entries[0].state, RosterState::Working);
    tokio::time::timeout(Duration::from_secs(2), proxy.front())
        .await
        .expect("Front() does not wait either")
        .expect("front");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn only_the_shell_speaks_for_the_person() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let w = world(vec![words("Hello there.")]);
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_on(&server, companion.clone(), who.clone())
        .await
        .expect("serving");
    as_shell(&who, &shell);

    let words_of_the_person = UserTurn {
        id: TurnId(1),
        text: "archive everything".into(),
        at: prov::UnixSeconds(0),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
    };
    let ask = AskWire {
        session: prov::SessionId::parse("s-1").expect("session"),
        turn: words_of_the_person.clone(),
        keep: keep_nothing(),
        parent_window: WindowKey::parse("w1").expect("window"),
        app: None,
    };
    let ask = serde_json::to_string(&ask).expect("json");
    let told = |turn: &UserTurn| serde_json::to_string(turn).expect("json");
    let agent = serde_json::to_string(&AgentRef::Worker { task: task("t-9") }).expect("json");

    let refused = CompanionProxy::new(&stranger).await.expect("proxy");
    assert!(refused.ask(&ask, &Details::new()).await.is_err());
    assert!(
        refused
            .told(&agent, "work", &told(&words_of_the_person))
            .await
            .is_err()
    );
    // Opening and closing a conversation are the person's too.
    let open = serde_json::to_string(&SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
        external: None,
        helper: None,
    })
    .expect("json");
    assert!(refused.open(&open).await.is_err());
    assert!(refused.close("s-1").await.is_err());
    assert!(companion.lock().await.roster().entries.is_empty());

    // The shell is heard: a turn said to a subagent shows on the roster at once.
    let proxy = CompanionProxy::new(&shell).await.expect("proxy");
    proxy
        .told(&agent, "work", &told(&words_of_the_person))
        .await
        .expect("the shell is heard");
    let opened: SessionOpened =
        serde_json::from_str(&proxy.open(&open).await.expect("the shell opens")).expect("json");
    proxy
        .close(opened.session.as_str())
        .await
        .expect("the shell closes");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_helpers_are_served_beside_the_conversation_to_the_shell_alone() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let hub = hub_of(&w);
    w.companion.set_desk(Arc::new(hub));
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_on(&server, companion.clone(), who.clone())
        .await
        .expect("serving");
    as_shell(&who, &shell);

    // Served as declared, on the object the conversation is on.
    let declared = introspection(Bus::Companion);
    let served = introspect(&shell, COMPANION_PATH).await;
    assert_eq!(
        block(&served, "org.quire.Companion1.Helpers"),
        block(&declared, "org.quire.Companion1.Helpers")
    );

    // The shell lists none, makes one, and hears that something changed.
    let helpers = CompanionHelpersProxy::new(&shell).await.expect("proxy");
    let mut changed = helpers.receive_changed().await.expect("signal");
    let none: HelpersView =
        serde_json::from_str(&helpers.list(0).await.expect("list")).expect("view");
    assert!(none.helpers.is_empty());
    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Unset,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let form = serde_json::to_string(&form).expect("json");
    let made: HelperReply =
        serde_json::from_str(&helpers.create(&form).await.expect("create")).expect("reply");
    let HelperReply::Made(id) = made else {
        panic!("a helper is made: {made:?}")
    };
    tokio::time::timeout(Duration::from_secs(5), changed.next())
        .await
        .expect("the change is told")
        .expect("a signal");
    let listed: HelpersView =
        serde_json::from_str(&helpers.list(0).await.expect("list")).expect("view");
    assert_eq!(listed.helpers.len(), 1);
    assert_eq!(listed.helpers[0].name.as_str(), "Research");

    // Nobody else speaks for the person here: not to read, not to change.
    let id_text = serde_json::to_value(&id)
        .expect("json")
        .as_str()
        .expect("text")
        .to_owned();
    let refused = CompanionHelpersProxy::new(&stranger).await.expect("proxy");
    assert!(refused.list(0).await.is_err());
    assert!(refused.card(&id_text).await.is_err());
    assert!(refused.create(&form).await.is_err());
    assert!(refused.pause(&id_text).await.is_err());
    assert!(refused.remove(&id_text).await.is_err());
    // The shell reads a card, and a helper that is not there is an error, not a guess.
    assert!(helpers.card(&id_text).await.is_ok());
    assert!(helpers.card("h-404").await.is_err());
}

/// Routines that hold nothing and make nothing: enough to see the calls reach the desk.
#[derive(Debug)]
struct EmptyRoutines;

impl RoutineDesk for EmptyRoutines {
    fn view(&self) -> RoutinesView {
        RoutinesView::default()
    }

    fn card(&self, _routine: RoutineId) -> Option<RoutineCardView> {
        None
    }

    fn create(&self, _form: RoutineForm) -> Pending<'_, RoutineReply> {
        Box::pin(async { RoutineReply::Refused("Nothing can be made here.".to_owned()) })
    }

    fn pause(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn resume(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn remove(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn helpers2_is_served_beside_helpers_with_sign_in_and_routines_to_the_shell_alone() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let hub = hub_of(&w);
    w.companion.set_desk(Arc::new(hub));
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_with_routines(
        &server,
        companion.clone(),
        who.clone(),
        std::path::PathBuf::from("/proc"),
        Some(Arc::new(EmptyRoutines)),
    )
    .await
    .expect("serving");
    as_shell(&who, &shell);

    // Served as declared, and the first version is still there as it was.
    let declared = introspection(Bus::Companion);
    let served = introspect(&shell, COMPANION_PATH).await;
    for name in [
        "org.quire.Companion1.Helpers",
        "org.quire.Companion1.Helpers2",
        "org.quire.Companion1.Terminals",
    ] {
        assert_eq!(block(&served, name), block(&declared, name));
    }

    let helpers = CompanionHelpers2Proxy::new(&shell).await.expect("proxy");
    let none: HelpersView =
        serde_json::from_str(&helpers.list(0).await.expect("list")).expect("view");
    assert!(none.helpers.is_empty());

    // A helper that runs no app has nothing to sign in to or out of, and says so in words.
    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Unset,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let form = serde_json::to_string(&form).expect("json");
    let made: HelperReply =
        serde_json::from_str(&helpers.create(&form).await.expect("create")).expect("reply");
    let HelperReply::Made(id) = made else {
        panic!("a helper is made: {made:?}")
    };
    let id_text = serde_json::to_value(&id)
        .expect("json")
        .as_str()
        .expect("text")
        .to_owned();
    for reply in [
        helpers.sign_in(&id_text, "browser").await.expect("sign in"),
        helpers.sign_out(&id_text).await.expect("sign out"),
    ] {
        let reply: HelperReply = serde_json::from_str(&reply).expect("reply");
        assert!(matches!(reply, HelperReply::Refused(_)), "{reply:?}");
    }

    // A helper that runs no app has no conversation to move, and says so in words.
    for reply in [
        helpers.open_in_terminal(&id_text).await.expect("open"),
        helpers.bring_back(&id_text).await.expect("back"),
    ] {
        let reply: HelperReply = serde_json::from_str(&reply).expect("reply");
        assert!(matches!(reply, HelperReply::Refused(_)), "{reply:?}");
    }
    // Conversations a terminal started: none yet, and one that is not there is refused.
    let terminals = CompanionTerminalsProxy::new(&shell).await.expect("proxy");
    let listed: Vec<TerminalSession> =
        serde_json::from_str(&terminals.sessions().await.expect("sessions")).expect("list");
    assert!(listed.is_empty());
    let gone: BroughtBack =
        serde_json::from_str(&terminals.bring_back("s-404").await.expect("bring back"))
            .expect("reply");
    assert!(matches!(gone, BroughtBack::Refused(_)), "{gone:?}");
    assert!(terminals.bring_back("not a session").await.is_err());

    // The routines reach their desk; one that is not there is an error, not a guess.
    let listed: RoutinesView =
        serde_json::from_str(&helpers.routines().await.expect("routines")).expect("view");
    assert!(listed.routines.is_empty());
    assert!(helpers.routine_card("r-1").await.is_err());
    assert!(helpers.routine_card("not a routine").await.is_err());
    helpers.pause_routine("r-1").await.expect("pause");
    helpers.resume_routine("r-1").await.expect("resume");
    helpers.remove_routine("r-1").await.expect("remove");

    // Nobody else speaks for the person here.
    let refused = CompanionHelpers2Proxy::new(&stranger).await.expect("proxy");
    assert!(refused.list(0).await.is_err());
    assert!(refused.sign_in(&id_text, "browser").await.is_err());
    assert!(refused.routines().await.is_err());
    assert!(refused.remove_routine("r-1").await.is_err());
    assert!(refused.open_in_terminal(&id_text).await.is_err());
    assert!(refused.bring_back(&id_text).await.is_err());
    let nobody = CompanionTerminalsProxy::new(&stranger).await.expect("proxy");
    assert!(nobody.sessions().await.is_err());
    assert!(nobody.bring_back("s-1").await.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_editor_bridge_reads_the_helpers_and_changes_nothing_and_a_stranger_reads_nothing() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, editor, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let hub = hub_of(&w);
    w.companion.set_desk(Arc::new(hub));
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    serve_with_routines(
        &server,
        companion,
        who.clone(),
        std::path::PathBuf::from("/proc"),
        Some(Arc::new(EmptyRoutines)),
    )
    .await
    .expect("serving");
    as_shell(&who, &shell);
    who.introduce_app(&editor, "org.quire.Acp", docket_dbus::callers::Role::Agent);

    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Unset,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let form = serde_json::to_string(&form).expect("json");
    let theirs = CompanionHelpers2Proxy::new(&shell).await.expect("proxy");
    let made: HelperReply =
        serde_json::from_str(&theirs.create(&form).await.expect("create")).expect("reply");
    let HelperReply::Made(id) = made else {
        panic!("a helper is made: {made:?}")
    };
    let id_text = serde_json::to_value(&id)
        .expect("json")
        .as_str()
        .expect("text")
        .to_owned();

    // The editor bridge can look, on either version of the interface.
    let helpers = CompanionHelpers2Proxy::new(&editor).await.expect("proxy");
    let listed: HelpersView =
        serde_json::from_str(&helpers.list(0).await.expect("list")).expect("view");
    assert_eq!(listed.helpers.len(), 1);
    helpers.card(&id_text).await.expect("card");
    let first = CompanionHelpersProxy::new(&editor).await.expect("proxy");
    first.list(0).await.expect("list");

    // It changes nothing: every other call is refused in words.
    let refused = |result: zbus::Result<_>| match result {
        Err(zbus::Error::MethodError(name, Some(text), _)) => {
            name.as_str() == "org.freedesktop.DBus.Error.AccessDenied" && text.contains("helpers")
        }
        _ => false,
    };
    assert!(refused(helpers.create(&form).await.map(drop)), "create");
    assert!(
        refused(helpers.edit(&id_text, &form).await.map(drop)),
        "edit"
    );
    assert!(refused(helpers.remove(&id_text).await), "remove");
    assert!(
        refused(helpers.post_job(&id_text, "Do it.", "").await.map(drop)),
        "post job"
    );
    assert!(
        refused(helpers.sign_in(&id_text, "browser").await.map(drop)),
        "sign in"
    );
    assert!(
        refused(helpers.open_in_terminal(&id_text).await.map(drop)),
        "open in terminal"
    );
    assert!(
        refused(helpers.bring_back(&id_text).await.map(drop)),
        "bring back"
    );
    let terminals = CompanionTerminalsProxy::new(&editor).await.expect("proxy");
    assert!(
        refused(terminals.sessions().await.map(drop)),
        "the terminals' conversations"
    );
    assert!(
        refused(helpers.sign_out(&id_text).await.map(drop)),
        "sign out"
    );
    assert!(refused(helpers.routines().await.map(drop)), "routines");
    assert!(
        refused(helpers.remove_routine("r-1").await),
        "routine write"
    );
    assert!(refused(first.pause(&id_text).await), "pause");

    // A process nothing names reads nothing.
    let nobody = CompanionHelpers2Proxy::new(&stranger).await.expect("proxy");
    assert!(nobody.list(0).await.is_err());
    assert!(nobody.card(&id_text).await.is_err());

    // The helper is still there.
    let after: HelpersView =
        serde_json::from_str(&theirs.list(0).await.expect("list")).expect("view");
    assert_eq!(after.helpers.len(), 1);
}

/// Routines that remember the forms they were asked to make.
#[derive(Debug, Default)]
struct RecordingRoutines {
    made: std::sync::Mutex<Vec<RoutineForm>>,
}

impl RoutineDesk for RecordingRoutines {
    fn view(&self) -> RoutinesView {
        RoutinesView::default()
    }

    fn card(&self, _routine: RoutineId) -> Option<RoutineCardView> {
        None
    }

    fn create(&self, form: RoutineForm) -> Pending<'_, RoutineReply> {
        self.made.lock().expect("made").push(form);
        Box::pin(async { RoutineReply::Made(RoutineId(1)) })
    }

    fn pause(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn resume(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }

    fn remove(&self, _routine: RoutineId) -> Pending<'_, ()> {
        Box::pin(async {})
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_proposed_routine_waits_for_the_shell_to_accept_or_decline_it() {
    let scratch = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(scratch.path());
    let (server, shell, editor, stranger) = (
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
        bus.connect().await,
    );
    let mut w = world(vec![]);
    let hub = hub_of(&w);
    w.companion.set_desk(Arc::new(hub.clone()));
    let companion = Arc::new(Mutex::new(w.companion));
    let who = who(&server, scratch.path());
    let routines = Arc::new(RecordingRoutines::default());
    serve_with_routines(
        &server,
        companion,
        who.clone(),
        std::path::PathBuf::from("/proc"),
        Some(routines.clone()),
    )
    .await
    .expect("serving");
    as_shell(&who, &shell);
    who.introduce_app(&editor, "org.quire.Acp", docket_dbus::callers::Role::Agent);

    let form = HelperForm {
        name: HelperName::parse("Research").expect("name"),
        role: Role::parse("Looks things up.").expect("role"),
        space: space("work"),
        model: HelperModel::Unset,
        apps: vec![app("org.quire.Mail")],
        ceiling: Ceiling::Look,
        steps: porter_core::Count(8),
        kind: Default::default(),
    };
    let helpers = CompanionHelpers2Proxy::new(&shell).await.expect("proxy");
    helpers
        .create(&serde_json::to_string(&form).expect("json"))
        .await
        .expect("create");
    let propose = |when: &str| RoutineAsk {
        helper: RoutineFor::Named("Research".to_owned()),
        goal: "Look up the news.".to_owned(),
        when: when.to_owned(),
        why: "You read the news every morning.".to_owned(),
        space: space("work"),
    };
    let proposals =
        |text: String| -> Vec<RoutineProposal> { serde_json::from_str(&text).expect("proposals") };

    // A time nobody can read, and a helper that is not there, are refused in words.
    assert!(matches!(
        hub.propose_routine(propose("whenever")).await,
        HelperReply::Refused(_)
    ));
    let nobody = RoutineAsk {
        helper: RoutineFor::Named("Nobody".to_owned()),
        ..propose("daily 08:00")
    };
    assert!(matches!(
        hub.propose_routine(nobody).await,
        HelperReply::Refused(_)
    ));
    assert!(proposals(helpers.routine_proposals().await.expect("none")).is_empty());

    // Two proposals are listed, and nothing is made before the shell answers.
    for when in ["daily 08:00", "every 2 hours"] {
        assert_eq!(hub.propose_routine(propose(when)).await, HelperReply::Done);
    }
    let listed = proposals(helpers.routine_proposals().await.expect("list"));
    assert_eq!(listed.len(), 2);
    assert_eq!(listed[0].id.to_string(), "p-1");
    assert_eq!(listed[0].why, "You read the news every morning.");
    assert!(routines.made.lock().expect("made").is_empty());

    // The editor bridge may look and may not answer; a stranger may do neither.
    let theirs = CompanionHelpers2Proxy::new(&editor).await.expect("proxy");
    assert_eq!(
        proposals(theirs.routine_proposals().await.expect("read")).len(),
        2
    );
    assert!(theirs.accept_routine_proposal("p-1").await.is_err());
    assert!(theirs.decline_routine_proposal("p-1").await.is_err());
    let nobody = CompanionHelpers2Proxy::new(&stranger).await.expect("proxy");
    assert!(nobody.routine_proposals().await.is_err());
    assert!(nobody.accept_routine_proposal("p-1").await.is_err());
    assert_eq!(
        proposals(helpers.routine_proposals().await.expect("list")).len(),
        2
    );

    // Accepting makes the routine, skipping a missed time, and takes it off the list.
    let reply: RoutineReply = serde_json::from_str(
        &helpers
            .accept_routine_proposal("p-1")
            .await
            .expect("accept"),
    )
    .expect("reply");
    assert_eq!(reply, RoutineReply::Made(RoutineId(1)));
    let made = routines.made.lock().expect("made").clone();
    assert_eq!(made.len(), 1);
    assert_eq!(made[0].missed, docket_core::routine::Missed::Skip);
    assert_eq!(made[0].goal, "Look up the news.");

    // Declining removes the other and makes nothing; a proposal already answered is not there.
    helpers
        .decline_routine_proposal("p-2")
        .await
        .expect("decline");
    assert!(proposals(helpers.routine_proposals().await.expect("list")).is_empty());
    assert_eq!(routines.made.lock().expect("made").len(), 1);
    let gone: RoutineReply = serde_json::from_str(
        &helpers
            .accept_routine_proposal("p-2")
            .await
            .expect("answered"),
    )
    .expect("reply");
    assert!(matches!(gone, RoutineReply::Refused(_)));
    assert!(helpers.accept_routine_proposal("nonsense").await.is_err());
}
