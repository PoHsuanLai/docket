//! Who may bring a stored session back by naming it: only its opener, the shell or the companion.
//! Anyone else is answered exactly as for a session that never existed.

mod support;

use docket_core::*;
use docket_fake::{FixedClock, ScriptedReviewer, ScriptedWriter, fake_router_on};
use docket_router::Router;
use docket_session::fake::MemoryLog;
use prov::{AgentRef, SessionId, UnixSeconds};
use std::sync::Arc;
use support::*;

type World = Router<docket_fake::FakeSeams>;

fn world(log: &Arc<MemoryLog>) -> World {
    fake_router_on(
        AgentConfig::default(),
        ScriptedReviewer::always_allow(),
        ScriptedWriter::failing(),
        FixedClock::at(UnixSeconds(0)),
        Arc::clone(log),
    )
    .expect("router")
}

fn field(name: &str) -> CallerId {
    caller(name, CallerRole::Field)
}

async fn turn(router: &World, who: &CallerId, session: &SessionId) -> IntentsReply {
    ask(
        router,
        who,
        IntentsRequest::SessionTurn {
            session: session.clone(),
            turn: TurnIn {
                text: "hello".into(),
                origin: Origin::Launcher,
                keep: ContextKeep {
                    query: Keep::Dropped,
                    results: Keep::Dropped,
                    selection: Keep::Dropped,
                    window: Keep::Dropped,
                },
                via: TurnVia::Typed,
            },
        },
    )
    .await
}

#[tokio::test]
async fn only_the_opener_restores_a_session_by_naming_it() {
    let log = Arc::new(MemoryLog::new());
    let (a, b) = (field("org.quire.A"), field("org.quire.B"));
    let router = world(&log);
    let reply = ask(
        &router,
        &a,
        IntentsRequest::SessionOpen(SessionOpen {
            space: space("work"),
            agent: AgentRef::User,
            parent: None,
        }),
    )
    .await;
    let IntentsReply::SessionOpened(opened) = reply else {
        panic!("open: {reply:?}")
    };
    assert!(matches!(
        turn(&router, &a, &opened.session).await,
        IntentsReply::TurnRecorded(_)
    ));
    drop(router);

    let again = world(&log);
    let unknown = SessionId::parse("s-9999").expect("id");
    let never = turn(&again, &b, &unknown).await;
    assert_eq!(never, IntentsReply::Refused(WireRefusal::NoSuchSession));
    let stranger = turn(&again, &b, &opened.session).await;
    assert_eq!(
        stranger, never,
        "a stored session answers as an unknown one"
    );
    assert!(
        !again
            .state
            .lock()
            .expect("lock")
            .sessions
            .contains_key(&opened.session),
        "the stranger revived nothing"
    );
    let owner = turn(&again, &a, &opened.session).await;
    assert!(matches!(owner, IntentsReply::TurnRecorded(_)), "{owner:?}");
}

#[tokio::test]
async fn the_shell_restores_any_stored_session() {
    let log = Arc::new(MemoryLog::new());
    let router = world(&log);
    let opened = open(&router, "work", AgentRef::Companion).await;
    drop(router);
    let again = world(&log);
    let shell = turn(&again, &launcher(), &opened.session).await;
    assert!(matches!(shell, IntentsReply::TurnRecorded(_)), "{shell:?}");
}
