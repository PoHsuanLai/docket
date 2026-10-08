//! A terminal speaks for the person on `Companion1` for the conversation only: it may open, ask
//! and close, it may not tell a subagent or press a card, and a process that is not in a
//! terminal's scope, or that owns a bus name of its own, may do none of it. The terminal is the
//! test process, placed in a fake proc root (`serve_on_rooted`), on a private bus.

use crate::support::bus::PrivateBus;
use crate::support::infer::words;
use crate::support::world::*;
use companion_wire::{AnswerPhase, AnswerWire, AskWire};
use companiond::serve_on_rooted;
use docket_core::*;
use docket_dbus::{CompanionAnswerProxy, CompanionProxy, Details};
use prov::AgentRef;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

fn place(root: &Path, leaf: &str) {
    let at = root.join(std::process::id().to_string());
    std::fs::create_dir_all(&at).expect("fake proc");
    std::fs::write(
        at.join("cgroup"),
        format!("0::/user.slice/user-1000.slice/user@1000.service/app.slice/{leaf}\n"),
    )
    .expect("cgroup");
}

fn open_json() -> String {
    serde_json::to_string(&SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
    })
    .expect("json")
}

fn denied<T: std::fmt::Debug>(result: zbus::Result<T>) -> bool {
    matches!(
        result,
        Err(zbus::Error::MethodError(ref name, ..))
            if name.as_str() == "org.freedesktop.DBus.Error.AccessDenied"
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_terminal_opens_asks_and_closes_but_neither_tells_nor_presses_a_card() {
    let scratch = tempfile::tempdir().expect("scratch");
    let proc_root = scratch.path().join("proc");
    place(&proc_root, "vte-spawn-1.scope");
    let bus = PrivateBus::start(scratch.path());
    let (server, client) = (bus.connect().await, bus.connect().await);
    let w = world(vec![words("Hello there.")]);
    let (launcher, hand) = (w.launcher, w.hand);
    let companion = Arc::new(Mutex::new(w.companion));
    serve_on_rooted(&server, companion.clone(), proc_root)
        .await
        .expect("serving");
    let proxy = CompanionProxy::new(&client).await.expect("proxy");

    let opened: SessionOpened =
        serde_json::from_str(&proxy.open(&open_json()).await.expect("open")).expect("opened");
    let turn = launcher
        .session_turn(
            opened.session.clone(),
            TurnIn {
                text: "hello".into(),
                origin: Origin::Cli,
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
            from: TurnSource::Terminal,
            via: TurnVia::Typed,
        },
        keep: keep_nothing(),
        parent_window: WindowKey::parse("w1").expect("window"),
        app: None,
    };
    let path = proxy
        .ask(&serde_json::to_string(&ask).expect("json"), &Details::new())
        .await
        .expect("a terminal may ask");
    let answers = CompanionAnswerProxy::builder(&client)
        .path(path)
        .expect("path")
        .cache_properties(zbus::proxy::CacheProperties::No)
        .build()
        .await
        .expect("answer proxy");
    let mut phase = AnswerPhase::Thinking;
    for _ in 0..100 {
        let view: AnswerWire =
            serde_json::from_str(&answers.view().await.expect("view")).expect("answer");
        phase = view.phase;
        if phase == AnswerPhase::Done {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(phase, AnswerPhase::Done);

    // What is the shell's alone stays the shell's.
    assert!(denied(answers.act("{}").await), "Act");
    let told = proxy
        .told(
            r#"{"kind":"companion"}"#,
            "work",
            r#"{"id":1,"text":"x","at":1,"from":{"kind":"terminal"},"via":"typed"}"#,
        )
        .await;
    assert!(denied(told), "Told");

    proxy.close(opened.session.as_str()).await.expect("close");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_process_that_is_not_a_terminal_or_owns_a_name_speaks_for_nobody() {
    let scratch = tempfile::tempdir().expect("scratch");
    let proc_root = scratch.path().join("proc");
    let bus = PrivateBus::start(scratch.path());
    let (server, client) = (bus.connect().await, bus.connect().await);
    let w = world(vec![]);
    let companion = Arc::new(Mutex::new(w.companion));
    serve_on_rooted(&server, companion, proc_root.clone())
        .await
        .expect("serving");
    let proxy = CompanionProxy::new(&client).await.expect("proxy");

    // No cgroup in the fake proc root: nothing says it is a terminal.
    assert!(denied(proxy.open(&open_json()).await), "no cgroup");
    // A service unit's scope is not a terminal's.
    place(&proc_root, "some.service");
    assert!(denied(proxy.open(&open_json()).await), "a unit");
    // A terminal's child that owns an application name is that application, not the person.
    place(&proc_root, "vte-spawn-1.scope");
    client
        .request_name("org.quire.Mail")
        .await
        .expect("a name of its own");
    assert!(denied(proxy.open(&open_json()).await), "owns a name");
}
