//! Parameters the router fills itself: the planner is not offered them and cannot send them, the
//! person's words come from the conversation's newest turn even when a screen run's session
//! makes the call, the app's history is read from memory about the conversation, and a read that
//! fails leaves the argument out without stopping the call.

use crate::recall::entry;
use crate::support::*;
use almanac_core::{
    BodyMode, EntriesPage, EntriesQuery, KindPattern, MemoryReply, MemoryRequest, Refusal,
};
use docket_core::*;
use docket_fake::{BoxFut, FakeMemory, HostedApp};
use docket_permissions_core::ActionMatch;
use docket_session::session_thing;
use docket_vocab::{ActionRef, Args, CallRequest, Origin, TargetValue, Value};
use docket_wire::{IntentsReply, IntentsRequest, SessionOpen};
use porter_core::Count;
use prov::{ActionName, AgentRef, Effect, Label, Labelled, SessionId, Source};
use std::sync::{Arc, Mutex};

const WATCHER: &str = "org.quire.Watcher";

/// An app with one read that takes the person's words and its own history, both filled.
const MANIFEST: &str = r#"
vocab = 1
app = "org.quire.Watcher"
entities = []

[[actions]]
name = "watcher.look"
label = "Look"
on = { kind = "nothing" }
effect = "read"
classes = []
undo = "not_undoable"
reach = "offered"
latency = "quick"
result = { kind = "nothing" }
keys = { kind = "none" }
lasting = "no"
dry_run = "none"

[[actions.params]]
name = "said"
label = "What the person said"
ty = { kind = "text", v = { max = 100000, lines = "many" } }
need = { kind = "optional" }
sink = "inert"
filled = { kind = "person_words" }

[[actions.params]]
name = "seen"
label = "Earlier attempts"
ty = { kind = "text", v = { max = 100000, lines = "many" } }
need = { kind = "optional" }
sink = "inert"
filled = { kind = "own_history", v = { kinds = "cua.*" } }
"#;

/// The app: it keeps the arguments of every call it is given.
#[derive(Debug, Default)]
struct Watcher {
    seen: Mutex<Vec<Args>>,
}

impl Watcher {
    fn calls(&self) -> Vec<Args> {
        self.seen.lock().expect("lock").clone()
    }
}

impl HostedApp for Watcher {
    fn perform(&self, inv: Invocation) -> BoxFut<Result<Outcome, AppRefusal>> {
        self.seen.lock().expect("lock").push(inv.args);
        Box::pin(async {
            Ok(Outcome {
                value: None,
                said: None,
                show: Preview::None,
                undo: Undoable::No,
                follow: Follow::Nothing,
            })
        })
    }
}

type Router = docket_router::Router<docket_fake::FakeSeams>;

/// A router whose memory answers `replies`, with the watcher installed.
fn world(replies: Vec<MemoryReply>) -> (Router, Arc<Watcher>) {
    let mut router = router();
    router.seams.memory = FakeMemory::answering(replies);
    let watcher = Arc::new(Watcher::default());
    router.seams.link.host(app(WATCHER), watcher.clone());
    router
        .state
        .lock()
        .expect("lock")
        .registry
        .insert(docket_manifest::parse(MANIFEST).expect("manifest"));
    (router, watcher)
}

fn look(args: Vec<(&str, Value)>) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app(WATCHER),
            name: ActionName::parse("watcher.look").expect("action"),
        },
        target: TargetValue::Nothing,
        args: args
            .into_iter()
            .map(|(k, v)| {
                (
                    param(k),
                    Labelled {
                        value: v,
                        label: trusted(),
                    },
                )
            })
            .collect(),
        origin: Origin::Companion,
    }
}

/// A conversation whose person has said two things, and a screen run working for it that has
/// heard a third of its own. Returns the conversation and the run.
async fn conversation_and_run(router: &Router) -> (SessionId, SessionId) {
    let front = open(router, "work", AgentRef::Companion).await;
    say(router, &front.session, "open the invoice").await;
    say(router, &front.session, "now send it to Accounting").await;
    let reply = ask(
        router,
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
    let IntentsReply::SessionOpened(run) = reply else {
        panic!("{reply:?}");
    };
    say(router, &run.session, "the run's own turn").await;
    let mut policy = wide_policy(&run.task, "work");
    policy.actions =
        std::collections::BTreeSet::from([ActionMatch::AppUpTo(app(WATCHER), Effect::Read)]);
    give_policy(router, &run.session, policy);
    (front.session, run.session)
}

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

fn screen() -> Label {
    Label::untrusted(
        Source::Screen { app: mail_app() },
        prov::DataClass::Mail,
        space("work"),
    )
}

fn page(entries: Vec<almanac_core::RecentEntry>) -> MemoryReply {
    MemoryReply::Entries(EntriesPage {
        entries,
        next: None,
    })
}

fn text_of(args: &Args, name: &str) -> Option<String> {
    match args.get(&param(name)).map(|a| &a.value) {
        Some(Value::Text(t)) => Some(t.clone()),
        _ => None,
    }
}

#[tokio::test]
async fn a_call_that_sends_a_filled_parameter_is_refused() {
    for name in ["said", "seen"] {
        let (router, watcher) = world(vec![]);
        let (_, run) = conversation_and_run(&router).await;
        let end = perform_in(
            &router,
            &run,
            look(vec![(name, Value::Text("mine".into()))]),
        )
        .await;
        assert_eq!(
            end,
            Err(CallRefusal::BadArgs {
                param: param(name),
                why: ArgFault::WrongType,
            }),
            "{name}"
        );
        assert!(watcher.calls().is_empty(), "{name}: the app was not called");
    }
}

#[tokio::test]
async fn the_persons_words_are_the_conversations_newest_turn_even_from_a_run() {
    let (router, watcher) = world(vec![]);
    let (_, run) = conversation_and_run(&router).await;
    perform_in(&router, &run, look(vec![]))
        .await
        .expect("the call runs");
    let calls = watcher.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(
        text_of(&calls[0], "said").as_deref(),
        Some("now send it to Accounting"),
        "the conversation's newest turn, not the run's own"
    );
    assert_eq!(
        calls[0][&param("said")].label,
        trusted(),
        "the person's own"
    );
}

#[tokio::test]
async fn the_apps_history_is_read_about_the_conversation_and_stays_untrusted() {
    let first = entry("cua.step", 1, screen(), Some(r#"{"n":1}"#), None);
    let second = entry("cua.step", 2, screen(), Some(r#"{"n":2}"#), None);
    let (router, watcher) = world(vec![page(vec![first, second])]);
    let (front, run) = conversation_and_run(&router).await;
    perform_in(&router, &run, look(vec![]))
        .await
        .expect("the call runs");

    let requests = router.seams.memory.requests();
    let [MemoryRequest::Entries(in_space, query)] = requests.as_slice() else {
        panic!("one Entries read: {requests:?}");
    };
    assert_eq!(*in_space, space("work"));
    assert_eq!(
        *query,
        EntriesQuery {
            kinds: vec![KindPattern::parse("cua.*").expect("pattern")],
            about: session_thing(&front),
            after: None,
            limit: Count(200),
            bodies: BodyMode::Json,
        },
        "about the conversation, not the run"
    );

    let calls = watcher.calls();
    assert_eq!(
        text_of(&calls[0], "seen").as_deref(),
        Some(r#"[{"n":1},{"n":2}]"#),
        "oldest first, bodies as stored"
    );
    assert_eq!(
        calls[0][&param("seen")].label.integrity,
        prov::Integrity::Untrusted
    );
}

#[tokio::test]
async fn a_history_that_cannot_be_read_is_left_out_and_the_call_runs() {
    let unanswered = [
        MemoryReply::Refused(Refusal::Unavailable),
        MemoryReply::Ok,
        page(vec![]),
    ];
    for reply in unanswered {
        let (router, watcher) = world(vec![reply.clone()]);
        let (_, run) = conversation_and_run(&router).await;
        perform_in(&router, &run, look(vec![]))
            .await
            .unwrap_or_else(|e| panic!("{reply:?}: {e:?}"));
        let calls = watcher.calls();
        assert_eq!(calls.len(), 1, "{reply:?}");
        assert!(!calls[0].contains_key(&param("seen")), "{reply:?}");
        assert!(calls[0].contains_key(&param("said")), "{reply:?}");
    }
}
