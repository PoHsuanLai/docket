//! What a terminal sees through `Session.Stored`: the sessions its own app opened, and the
//! conversations a terminal started through the companion (`Opening::started_from`), and nothing
//! else. Everything else is answered as a session the log does not hold.

use crate::support::*;
use docket_core::*;
use docket_fake::FakeSeams;
use docket_router::Router;
use prov::{AgentRef, SessionId};

fn scope(name: &str) -> StartedFrom {
    StartedFrom::Terminal(TerminalScope::from_cgroup(name).expect("terminal scope"))
}

/// The companion opens a conversation, naming the terminal it came from (or none).
async fn companion_opens(router: &Router<FakeSeams>, from: Option<StartedFrom>) -> SessionId {
    opens(
        router,
        &caller("org.quire.Companiond", CallerRole::Companion),
        from,
    )
    .await
}

async fn opens(router: &Router<FakeSeams>, who: &CallerId, from: Option<StartedFrom>) -> SessionId {
    let reply = ask(
        router,
        who,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: from,
        }),
    )
    .await;
    match reply {
        IntentsReply::SessionOpened(opened) => opened.session,
        other => panic!("open: {other:?}"),
    }
}

fn stored(ask: StoredAsk) -> IntentsRequest {
    IntentsRequest::SessionStored { ask }
}

async fn listed(router: &Router<FakeSeams>, who: &CallerId) -> Vec<SessionId> {
    match ask(router, who, stored(StoredAsk::List)).await {
        IntentsReply::Stored(StoredView::Sessions(all)) => all,
        other => panic!("{other:?}"),
    }
}

async fn rows_reply(router: &Router<FakeSeams>, who: &CallerId, s: &SessionId) -> IntentsReply {
    let rows = StoredAsk::Rows {
        session: s.clone(),
        from: None,
        size: 10,
    };
    ask(router, who, stored(rows)).await
}

fn gone() -> IntentsReply {
    IntentsReply::Refused(WireRefusal::NoSuchSession)
}

#[tokio::test]
async fn a_terminal_lists_loads_and_forks_a_conversation_a_terminal_started() {
    let router = router();
    let started = companion_opens(&router, Some(scope("vte-spawn-1.scope"))).await;
    say_as(&router, &cli(), &started, "what is on my calendar").await;

    assert_eq!(
        listed(&router, &cli()).await,
        std::slice::from_ref(&started)
    );
    let IntentsReply::Stored(StoredView::Rows { rows, .. }) =
        rows_reply(&router, &cli(), &started).await
    else {
        panic!("rows")
    };
    assert!(
        rows[0].json.contains("vte-spawn-1.scope"),
        "{}",
        rows[0].json
    );
    let fork = StoredAsk::Fork {
        session: started.clone(),
        at: 1,
    };
    let IntentsReply::Stored(StoredView::Forked(child)) = ask(&router, &cli(), stored(fork)).await
    else {
        panic!("fork")
    };
    // The child keeps where it started, so the terminal lists it too.
    assert_eq!(listed(&router, &cli()).await, [started, child]);
}

#[tokio::test]
async fn any_terminal_sees_it_not_only_the_same_scope() {
    let router = router();
    let started = companion_opens(&router, Some(scope("vte-spawn-1.scope"))).await;
    let other = caller("org.quire.OtherDo", CallerRole::Cli);
    assert_eq!(listed(&router, &other).await, [started]);
}

#[tokio::test]
async fn a_companion_session_the_launcher_started_is_not_shown_to_a_terminal() {
    let router = router();
    let shell = caller("org.quire.Shell", CallerRole::Launcher);
    let from_launcher = opens(&router, &shell, None).await;
    let plain = companion_opens(&router, None).await;
    for hidden in [&from_launcher, &plain] {
        assert_eq!(rows_reply(&router, &cli(), hidden).await, gone());
        let fork = StoredAsk::Fork {
            session: hidden.clone(),
            at: 0,
        };
        assert_eq!(ask(&router, &cli(), stored(fork)).await, gone());
    }
    assert_eq!(listed(&router, &cli()).await, Vec::<SessionId>::new());
}

#[tokio::test]
async fn an_editor_session_is_not_shown_to_a_terminal() {
    let router = router();
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    let theirs = opens(&router, &zed, None).await;
    assert_eq!(listed(&router, &cli()).await, Vec::<SessionId>::new());
    assert_eq!(rows_reply(&router, &cli(), &theirs).await, gone());
}

#[tokio::test]
async fn only_the_companion_can_say_a_terminal_started_it() {
    let router = router();
    let claimed = scope("vte-spawn-1.scope");
    let by_launcher = opens(
        &router,
        &caller("org.quire.Shell", CallerRole::Launcher),
        Some(claimed.clone()),
    )
    .await;
    let by_editor = opens(
        &router,
        &caller("org.zed.Zed", CallerRole::Editor),
        Some(claimed),
    )
    .await;
    assert_eq!(listed(&router, &cli()).await, Vec::<SessionId>::new());
    for s in [by_launcher, by_editor] {
        assert_eq!(rows_reply(&router, &cli(), &s).await, gone());
    }
}

#[tokio::test]
async fn an_editor_still_does_not_see_a_terminal_started_conversation() {
    let router = router();
    let started = companion_opens(&router, Some(scope("tmux-spawn-9.scope"))).await;
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    assert_eq!(rows_reply(&router, &zed, &started).await, gone());
    assert_eq!(listed(&router, &zed).await, Vec::<SessionId>::new());
}
