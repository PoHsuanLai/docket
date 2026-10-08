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
