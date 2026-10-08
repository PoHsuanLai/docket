//! `.Session.Stored`: the log read through the router. A caller lists and reads only the
//! sessions it may bring back; an editor sees what its own app opened.

use crate::support::*;
use docket_core::*;
use prov::AgentRef;

fn rows(reply: IntentsReply) -> (Vec<StoredRow>, Option<u64>) {
    match reply {
        IntentsReply::Stored(StoredView::Rows { rows, next }) => (rows, next),
        other => panic!("{other:?}"),
    }
}

fn sessions(reply: IntentsReply) -> Vec<prov::SessionId> {
    match reply {
        IntentsReply::Stored(StoredView::Sessions(all)) => all,
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn an_editor_lists_and_reads_its_own_sessions_and_nobody_elses() {
    let router = router();
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    let helix = caller("org.helix.Helix", CallerRole::Editor);
    let mine = open_as(&router, &zed, "work", AgentRef::Companion).await;
    say_as(&router, &zed, &mine.session, "hello").await;
    let theirs = open_as(&router, &helix, "work", AgentRef::Companion).await;
    say_as(&router, &helix, &theirs.session, "hi").await;

    let listed = sessions(
        ask(
            &router,
            &zed,
            IntentsRequest::SessionStored {
                ask: StoredAsk::List,
            },
        )
        .await,
    );
    assert_eq!(listed, std::slice::from_ref(&mine.session));

    let page = ask(
        &router,
        &zed,
        IntentsRequest::SessionStored {
            ask: StoredAsk::Rows {
                session: mine.session.clone(),
                from: None,
                size: 1,
            },
        },
    )
    .await;
    let (first, next) = rows(page);
    assert_eq!(first.len(), 1);
    assert_eq!(next, Some(1), "the page ends where the next begins");
    assert!(first[0].json.contains("opened"), "{}", first[0].json);

    let refused = ask(
        &router,
        &zed,
        IntentsRequest::SessionStored {
            ask: StoredAsk::Rows {
                session: theirs.session.clone(),
                from: None,
                size: 10,
            },
        },
    )
    .await;
    assert_eq!(refused, IntentsReply::Refused(WireRefusal::NoSuchSession));

    // The shell sees both.
    let all = sessions(
        ask(
            &router,
            &caller("org.quire.Shell", CallerRole::Launcher),
            IntentsRequest::SessionStored {
                ask: StoredAsk::List,
            },
        )
        .await,
    );
    assert_eq!(all.len(), 2);
}

#[tokio::test]
async fn an_app_without_the_role_cannot_read_the_log() {
    let router = router();
    let reply = ask(
        &router,
        &caller("org.quire.Mail", CallerRole::App),
        IntentsRequest::SessionStored {
            ask: StoredAsk::List,
        },
    )
    .await;
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
}

#[tokio::test]
async fn an_editor_closes_and_speaks_only_in_sessions_its_own_app_opened() {
    let router = router();
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    let helix = caller("org.helix.Helix", CallerRole::Editor);
    let theirs = open_as(&router, &helix, "work", AgentRef::Companion).await;
    let turn = IntentsRequest::SessionTurn {
        session: theirs.session.clone(),
        turn: TurnIn {
            text: "hi".into(),
            origin: Origin::InWindowField,
            keep: ContextKeep {
                query: Keep::Dropped,
                results: Keep::Dropped,
                selection: Keep::Dropped,
                window: Keep::Dropped,
            },
            via: TurnVia::Typed,
        },
    };
    assert_eq!(
        ask(&router, &zed, turn).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    let close = IntentsRequest::SessionClose {
        session: theirs.session.clone(),
    };
    assert_eq!(
        ask(&router, &zed, close.clone()).await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    assert_eq!(ask(&router, &helix, close).await, IntentsReply::Done);
}

fn forked(reply: IntentsReply) -> prov::SessionId {
    match reply {
        IntentsReply::Stored(StoredView::Forked(child)) => child,
        other => panic!("{other:?}"),
    }
}

fn fork_at(session: &prov::SessionId, at: u64) -> IntentsRequest {
    IntentsRequest::SessionStored {
        ask: StoredAsk::Fork {
            session: session.clone(),
            at,
        },
    }
}

#[tokio::test]
async fn a_terminal_reads_the_log_by_the_same_opener_rule() {
    let router = router();
    // The terminal's own app opened one (the launcher role is what may open; the rule reads the
    // app's name), the shell another.
    let terminal = caller("org.quire.Do", CallerRole::Cli);
    let mine = open_as(
        &router,
        &caller("org.quire.Do", CallerRole::Launcher),
        "work",
        AgentRef::Companion,
    )
    .await;
    let shells = open_as(
        &router,
        &caller("org.quire.Shell", CallerRole::Launcher),
        "work",
        AgentRef::Companion,
    )
    .await;
    let listed = sessions(
        ask(
            &router,
            &terminal,
            IntentsRequest::SessionStored {
                ask: StoredAsk::List,
            },
        )
        .await,
    );
    assert_eq!(listed, std::slice::from_ref(&mine.session));
    let page = |session: &prov::SessionId| IntentsRequest::SessionStored {
        ask: StoredAsk::Rows {
            session: session.clone(),
            from: None,
            size: 10,
        },
    };
    assert!(matches!(
        ask(&router, &terminal, page(&mine.session)).await,
        IntentsReply::Stored(StoredView::Rows { .. })
    ));
    assert_eq!(
        ask(&router, &terminal, page(&shells.session)).await,
        IntentsReply::Refused(WireRefusal::NoSuchSession)
    );
}

#[tokio::test]
async fn a_fork_is_written_by_the_router_for_whoever_may_bring_the_parent_back() {
    let router = router();
    let zed = caller("org.zed.Zed", CallerRole::Editor);
    let helix = caller("org.helix.Helix", CallerRole::Editor);
    let mine = open_as(&router, &zed, "work", AgentRef::Companion).await;
    say_as(&router, &zed, &mine.session, "hello").await;

    let child = forked(ask(&router, &zed, fork_at(&mine.session, 1)).await);
    assert_eq!(child.as_str(), format!("{}-f1", mine.session));
    // Its opener is the parent's: the editor lists it, another editor does not.
    let list = IntentsRequest::SessionStored {
        ask: StoredAsk::List,
    };
    assert_eq!(
        sessions(ask(&router, &zed, list.clone()).await),
        [mine.session.clone(), child.clone()]
    );
    assert_eq!(sessions(ask(&router, &helix, list).await), []);
    let (rows, _) = rows(
        ask(
            &router,
            &zed,
            IntentsRequest::SessionStored {
                ask: StoredAsk::Rows {
                    session: child.clone(),
                    from: None,
                    size: 10,
                },
            },
        )
        .await,
    );
    assert_eq!(rows.len(), 2, "the opening and the turn");
    assert!(rows[0].json.contains("forked_from"), "{}", rows[0].json);

    // Another editor's fork is answered as a session the log does not hold, and a position past
    // the end is malformed. Neither writes anything.
    assert_eq!(
        ask(&router, &helix, fork_at(&mine.session, 1)).await,
        IntentsReply::Refused(WireRefusal::NoSuchSession)
    );
    assert_eq!(
        ask(&router, &zed, fork_at(&mine.session, 99)).await,
        IntentsReply::Refused(WireRefusal::Malformed)
    );
    let second = forked(ask(&router, &zed, fork_at(&mine.session, 0)).await);
    assert_eq!(second.as_str(), format!("{}-f2", mine.session));
}
