//! The signals that say something changed inside the router: the difference between two looks at
//! its state (`changes`), and what a running daemon says on a private bus when a manifest file
//! appears, a task starts, the breaker pauses a session and a search has late hits. Signals are
//! content-free: a name, a count, an id.

use crate::support::world::{World, everything_config, mail_router};
use docket_core::*;
use docket_fake::fake_router;
use docket_router::{Router, SessionState, Taint};
use intentd::{Changed, changes, marks_of};
use porter_core::AppName;
use prov::{Actor, AgentRef, SpaceId};

fn router() -> Router<docket_fake::FakeSeams> {
    fake_router(AgentConfig::default()).expect("router")
}

fn files() -> AppName {
    AppName::parse("org.quire.Files").expect("app")
}

fn companion() -> CallerId {
    crate::support::world::caller("org.quire.Companiond", &[CallerRole::Companion])
}

async fn open(router: &Router<docket_fake::FakeSeams>, agent: AgentRef) -> SessionOpened {
    let reply = router
        .handle(
            &companion(),
            IntentsRequest::SessionOpen(SessionOpen {
                space: SpaceId::parse("work").expect("space"),
                agent,
                parent: None,
            }),
        )
        .await;
    let IntentsReply::SessionOpened(opened) = reply else {
        panic!("{reply:?}")
    };
    opened
}

#[test]
fn nothing_changed_says_nothing() {
    let router = router();
    let first = marks_of(&router);
    assert_eq!(changes(&first, &marks_of(&router)), vec![]);
}

#[test]
fn a_manifest_installed_replaced_or_removed_is_a_manifest_change() {
    let router = router();
    let installed = marks_of(&router);
    let registry =
        |f: &dyn Fn(&mut docket_router::RouterState)| f(&mut router.state.lock().expect("lock"));

    registry(&|st| {
        st.registry.remove(&files());
    });
    let removed = marks_of(&router);
    assert_eq!(
        changes(&installed, &removed),
        vec![Changed::Manifest(files())]
    );

    registry(&|st| {
        st.registry
            .insert(docket_fake::files_manifest().expect("manifest"))
    });
    let again = marks_of(&router);
    assert_eq!(changes(&removed, &again), vec![Changed::Manifest(files())]);
    assert_eq!(
        changes(&installed, &again),
        vec![],
        "the same manifest is not a change"
    );

    // The same app with a different declaration is a change.
    let edited = docket_fake::FILES_MANIFEST.replace("Read", "Look at");
    registry(&|st| {
        st.registry
            .insert(docket_router::parse(&edited).expect("manifest"))
    });
    assert_eq!(
        changes(&again, &marks_of(&router)),
        vec![Changed::Manifest(files())]
    );
}

#[tokio::test]
async fn a_row_in_the_journal_or_a_change_of_one_is_a_journal_change_with_the_row_count() {
    let router = router();
    let before = marks_of(&router);
    let id = router.state.lock().expect("lock").journal.record(
        prov::UnixSeconds(1),
        Actor::Cli,
        ActionRef {
            app: AppName::parse("org.quire.Mail").expect("app"),
            name: prov::ActionName::parse("mail.thread.archive").expect("action"),
        },
        LabelText::parse("Archived 1 thread").expect("words"),
        UndoToken::parse("u-1").expect("token"),
        None,
        None,
    );
    let one = marks_of(&router);
    assert_eq!(changes(&before, &one), vec![Changed::Journal(1)]);
    router
        .state
        .lock()
        .expect("lock")
        .journal
        .mark(id, UndoState::Undone { by: Actor::Cli });
    let undone = marks_of(&router);
    assert_eq!(
        changes(&one, &undone),
        vec![Changed::Journal(1)],
        "the same number of rows, one of them changed"
    );
}

#[tokio::test]
async fn a_session_the_breaker_pauses_is_said_once_and_again_after_it_is_resumed_and_paused() {
    let router = router();
    let front = open(&router, AgentRef::Companion).await;
    let before = marks_of(&router);
    let pause = |on: bool| {
        let mut st = router.state.lock().expect("lock");
        let record = st.sessions.get_mut(&front.session).expect("session");
        record.state = if on {
            SessionState::Paused {
                taint: Taint::Clean,
                trip: BreakerTrip::Consecutive,
            }
        } else {
            SessionState::Open(Taint::Clean)
        };
    };
    pause(true);
    let paused = marks_of(&router);
    assert_eq!(
        changes(&before, &paused),
        vec![Changed::Breaker(front.session.clone())]
    );
    assert_eq!(changes(&paused, &marks_of(&router)), vec![], "said once");
    pause(false);
    let resumed = marks_of(&router);
    assert_eq!(changes(&paused, &resumed), vec![]);
    pause(true);
    assert_eq!(
        changes(&resumed, &marks_of(&router)),
        vec![Changed::Breaker(front.session)]
    );
}

#[tokio::test]
async fn a_message_for_an_agent_is_an_arrival_for_that_agent_only_while_it_waits() {
    let router = router();
    let front = open(&router, AgentRef::Companion).await;
    let worker_agent = AgentRef::Worker {
        task: prov::TaskId::parse("t-77").expect("task"),
    };
    open(&router, worker_agent.clone()).await;
    let before = marks_of(&router);
    let reply = router
        .handle(
            &companion(),
            IntentsRequest::MessageSend {
                session: front.session.clone(),
                draft: MessageDraft {
                    to: prov::Address::new(
                        worker_agent.clone(),
                        SpaceId::parse("work").expect("space"),
                    ),
                    thread: None,
                    in_reply_to: None,
                    kind: prov::MessageKind::Note,
                    parts: vec![DraftPart::Text(prov::MessageText::new("psst"))],
                },
            },
        )
        .await;
    assert!(matches!(reply, IntentsReply::Delivered(_)), "{reply:?}");
    let after = marks_of(&router);
    assert_eq!(
        changes(&before, &after),
        vec![Changed::Arrived(worker_agent.clone())]
    );

    // Reading the inbox takes the message out: that is not an arrival.
    router
        .handle(
            &companion(),
            IntentsRequest::MessageInbox(InboxAsk {
                agent: worker_agent,
                after: None,
            }),
        )
        .await;
    assert_eq!(changes(&after, &marks_of(&router)), vec![]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn late_hits_come_to_the_asker_alone_as_the_hits_signal() {
    let router = mail_router();
    // Mail's threads are not indexed here, so a search has to ask the app.
    let unindexed =
        docket_fake::MAIL_MANIFEST.replace("index = \"indexed\"", "index = \"not_indexed\"");
    router
        .state
        .lock()
        .expect("lock")
        .registry
        .insert(docket_router::parse(&unindexed).expect("manifest"));
    let world = World::serving(router, everything_config()).await;
    let (_, asker) = world.client(&[crate::support::world::EVERYTHING]).await;
    let (_, bystander) = world.client(&["org.quire.Bystander"]).await;

    let mine = docket_dbus::SearchProxy::new(&asker).await.expect("proxy");
    let theirs = docket_dbus::SearchProxy::new(&bystander)
        .await
        .expect("proxy");
    let mut late = mine.receive_hits().await.expect("subscribed");
    let mut not_for_me = theirs.receive_hits().await.expect("subscribed");

    let scope = serde_json::to_string(&SearchScope::Everything).expect("scope");
    let at_once = mine.query("Invoice", &scope, 7).await.expect("the answer");
    let indexed: Vec<Hit> = serde_json::from_str(&at_once).expect("hits");
    assert!(
        indexed.is_empty(),
        "nothing is indexed; the app is asked: {indexed:?}"
    );

    use futures_util::StreamExt;
    let signal = tokio::time::timeout(std::time::Duration::from_secs(5), late.next())
        .await
        .expect("late hits arrive")
        .expect("a signal");
    let args = signal.args().expect("args");
    assert_eq!(*args.generation(), 7);
    let hits: Vec<Hit> = serde_json::from_str(args.hits()).expect("hits");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].entity.id.key.as_str(), "t1");

    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(300), not_for_me.next())
            .await
            .is_err(),
        "somebody else's search results are not for the bystander"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_caller_that_may_not_search_gets_no_hits_early_or_late() {
    // The reader may resolve a handle and do nothing else: searching is not its business.
    let mut config = everything_config();
    config.roles.insert(
        CallerRole::Reader,
        vec![AppName::parse("org.quire.Readerish").expect("app")],
    );
    config.roles.retain(|role, _| *role == CallerRole::Reader);
    let world = World::serving(mail_router(), config).await;
    let (_, reader) = world.client(&["org.quire.Readerish"]).await;
    let proxy = docket_dbus::SearchProxy::new(&reader).await.expect("proxy");
    let scope = serde_json::to_string(&SearchScope::Everything).expect("scope");
    let refused = proxy.query("Invoice", &scope, 1).await;
    assert!(refused.is_err(), "{refused:?}");
}
