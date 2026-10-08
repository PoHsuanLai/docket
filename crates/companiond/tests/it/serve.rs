//! `org.quire.Companion1` on a private bus: served as declared, an ask answered at once with the
//! answer's object path while the loop runs behind it, the roster and the front task read without
//! waiting for the loop, and the answer object's view.

use crate::support::bus::PrivateBus;
use crate::support::infer::{Say, words};
use crate::support::world::*;
use companion_wire::{AnswerPhase, AnswerWire, AskWire, FrontTask};
use companiond::serve_on;
use docket_core::*;
use docket_dbus::{
    Bus, COMPANION_BUS, COMPANION_PATH, CompanionAnswerProxy, CompanionProxy, Details,
    introspection,
};
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
    serve_on(&server, companion.clone()).await.expect("serving");
    // The client is the shell: only it speaks for the person.
    client
        .request_name("org.quire.Shell")
        .await
        .expect("the shell's name");

    let proxy = CompanionProxy::new(&client).await.expect("proxy");
    let mut added = proxy.receive_answer_added().await.expect("signal");

    // Open a front conversation over the bus.
    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
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
    serve_on(&server, companion.clone()).await.expect("serving");
    // The client is the shell: only it speaks for the person.
    client
        .request_name("org.quire.Shell")
        .await
        .expect("the shell's name");
    let proxy = CompanionProxy::new(&client).await.expect("proxy");

    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
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
    serve_on(&server, companion.clone()).await.expect("serving");
    shell.request_name("org.quire.Shell").await.expect("name");

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
