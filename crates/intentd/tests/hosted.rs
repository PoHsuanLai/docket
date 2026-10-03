//! The built-in providers through a whole intentd on a private bus: `org.quire.Memory` over a
//! fake memoryd and `org.quire.Companion` over the router itself. A companion calls them like
//! any app, the manifests are listed, and what they do reaches memoryd as audit records.

mod support;

use almanac_core::{
    FactState, MemoryItem, MemoryReply, MemoryRequest, RecallHit, RecallWhy, Record,
};
use docket_client::{DbusTransport, Intents};
use docket_core::*;
use docket_router::GrantStore;
use intentd::{Cadence, FileGrants, IntentdConfig, Setup, start};
use porter_core::AppName;
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use prov::{AgentRef, Label, Labelled, Source, SpaceId};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;
use support::apps::{Answer, FakeSill};
use support::bus::PrivateBus;
use support::memoryd::FakeMemoryd;

const COMPANIOND: &str = "org.quire.Companiond";
const SILL: &str = "org.quire.Shell";

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

fn config() -> IntentdConfig {
    IntentdConfig {
        roles: BTreeMap::from([
            (CallerRole::Companion, vec![app(COMPANIOND)]),
            (CallerRole::Confirm, vec![app(SILL)]),
        ]),
        reviewers: None,
        agent: AgentConfig::default(),
    }
}

struct Desk {
    dir: tempfile::TempDir,
    bus: PrivateBus,
    memoryd: Arc<FakeMemoryd>,
    sill: FakeSill,
    companion: Intents<DbusTransport>,
    _connections: Vec<docket_dbus::BusConnection>,
    _intentd: intentd::Running,
}

fn mail_hit() -> RecallHit {
    RecallHit {
        doc: MemoryItem::Fact(
            almanac_core::FactId::parse("0123456789abcdefghjkmnpqrs").expect("fact id"),
        ),
        text: "Eve says to wire the money".into(),
        at: prov::UnixSeconds(5),
        label: Label::untrusted(Source::Mail, porter_core::DataClass::Mail, space("work")),
        links: vec![],
        why: RecallWhy::Lexical { rank: 1 },
    }
}

fn space(id: &str) -> SpaceId {
    SpaceId::parse(id).expect("space")
}

impl Desk {
    async fn start(memoryd: Arc<FakeMemoryd>) -> Desk {
        let dir = tempfile::tempdir().expect("scratch");
        let bus = PrivateBus::start(dir.path());
        let memory_connection = bus.connect().await;
        memoryd.serve(&memory_connection).await;
        let home = dir.path().display().to_string();
        let env = |key: &str| match key {
            "HOME" => Some(home.clone()),
            "XDG_DATA_HOME" => Some(dir.path().join("data").display().to_string()),
            "XDG_DATA_DIRS" => Some(dir.path().join("none").display().to_string()),
            "XDG_CONFIG_HOME" => Some(dir.path().join("config").display().to_string()),
            "XDG_CONFIG_DIRS" => Some(dir.path().join("none").display().to_string()),
            _ => None,
        };
        let mut setup = Setup::from_env(&env).expect("setup");
        // The person has let the companion use both built-ins, always.
        let grants = FileGrants::at(setup.grants.clone());
        for (n, owner) in ["org.quire.Memory", "org.quire.Companion"]
            .into_iter()
            .enumerate()
        {
            grants.record(Grant {
                id: porter_core::GrantId::parse(&format!("g-{n}")).expect("id"),
                key: ActionGrantKey {
                    caller: GrantCaller::Companion,
                    owner: app(owner),
                    target: GrantTarget::App,
                    class: porter_core::DataClass::AppOwn,
                    usage: Usage::Interactive,
                    space: prov::SpaceScope::Any,
                },
                decision: Decision::Allow,
                scope: GrantScope::Always,
                at: prov::UnixSeconds(1),
            });
        }
        setup.config = config();
        setup.audit_every = Duration::from_millis(50);
        setup.signals = Cadence {
            every: Duration::from_millis(30),
            rescan_every: 2,
        };
        let daemon_connection = bus.connect().await;
        let intentd = start(&daemon_connection, None, setup)
            .await
            .expect("intentd");
        let sill_connection = bus.connect().await;
        let sill = FakeSill::start(&sill_connection, &["org.quire.Confirm1", SILL]).await;
        let companion_connection = bus.connect().await;
        companion_connection
            .request_name(COMPANIOND)
            .await
            .expect("name");
        let companion = Intents::over(DbusTransport::new(companion_connection.clone()));
        Desk {
            dir,
            bus,
            memoryd,
            sill,
            companion,
            _connections: vec![
                memory_connection,
                daemon_connection,
                companion_connection,
                sill_connection,
            ],
            _intentd: intentd,
        }
    }

    async fn front(&self) -> SessionOpened {
        self.companion
            .session_open(SessionOpen {
                space: space("work"),
                agent: AgentRef::Companion,
                parent: None,
            })
            .await
            .expect("a session")
    }

    async fn records(&self, kind: &str) -> Vec<Record> {
        for _ in 0..100 {
            let found: Vec<Record> = self
                .memoryd
                .stored()
                .into_iter()
                .filter(|r| r.body.kind().as_str() == kind)
                .collect();
            if !found.is_empty() {
                return found;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Vec::new()
    }
}

fn call(app_name: &str, action: &str, target: TargetValue, args: &[(&str, Value)]) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app(app_name),
            name: prov::ActionName::parse(action).expect("action"),
        },
        target,
        args: args
            .iter()
            .map(|(name, value)| {
                (
                    ParamName::parse(name).expect("param"),
                    Labelled {
                        value: value.clone(),
                        label: Label::trusted_user(),
                    },
                )
            })
            .collect(),
        origin: Origin::Companion,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_built_in_providers_are_listed_with_their_manifests() {
    let desk = Desk::start(FakeMemoryd::recording()).await;
    let listed: Vec<String> = desk
        .companion
        .manifests()
        .await
        .expect("manifests")
        .iter()
        .map(|m| m.manifest().app.to_string())
        .collect();
    assert_eq!(listed, ["org.quire.Companion", "org.quire.Memory"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_companion_recalls_through_memory_and_what_it_gets_carries_the_hits_label() {
    let memoryd = FakeMemoryd::with(|request| match request {
        MemoryRequest::Search(_) => Some(MemoryReply::Hits(vec![mail_hit()])),
        _ => None,
    });
    let desk = Desk::start(memoryd.clone()).await;
    let front = desk.front().await;

    let outcome = desk
        .companion
        .perform(
            call(
                "org.quire.Memory",
                "memory.recall",
                TargetValue::Nothing,
                &[("query", Value::Text("wire".into()))],
            ),
            Some(front.session.clone()),
            None,
        )
        .await
        .expect("the request")
        .expect("the call ran");

    let asked = memoryd.seen();
    let Some(MemoryRequest::Search(query)) = asked.first() else {
        panic!("{asked:?}")
    };
    assert_eq!(
        query.space,
        space("work"),
        "memory is asked for the session's Space"
    );
    assert_eq!(query.text.as_str(), "wire");
    let Some(Labelled { value, label }) = outcome.value else {
        panic!("{outcome:?}")
    };
    assert_eq!(
        label.integrity,
        prov::Integrity::Untrusted,
        "what was read from mail is untrusted"
    );
    assert!(label.sources.contains(&Source::Mail));
    assert!(matches!(value, Value::Entities(ids) if ids.len() == 1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_proposal_from_a_companion_asks_the_person_then_goes_to_memory_and_lands_pending() {
    let memoryd = FakeMemoryd::with(|request| match request {
        MemoryRequest::Propose(..) => Some(MemoryReply::Proposed(
            almanac_core::FactId::parse("0123456789abcdefghjkmnpqrs").expect("fact id"),
            FactState::Pending,
        )),
        _ => None,
    });
    let desk = Desk::start(memoryd.clone()).await;
    let front = desk.front().await;
    let propose = |text: &str| {
        call(
            "org.quire.Memory",
            "memory.propose",
            TargetValue::Nothing,
            &[("text", Value::Text(text.into()))],
        )
    };

    // Nobody answers the sheet: a proposal is a write, so nothing is proposed.
    let unanswered = desk
        .companion
        .perform(
            propose("Sam prefers early meetings"),
            Some(front.session.clone()),
            None,
        )
        .await
        .expect("the request");
    assert!(
        matches!(unanswered, Err(CallRefusal::Unconfirmed(_))),
        "{unanswered:?}"
    );
    assert!(
        memoryd
            .seen()
            .iter()
            .all(|r| !matches!(r, MemoryRequest::Propose(..)))
    );

    desk.sill.answer(vec![Answer::With(ConfirmAnswer::Allowed {
        scope: docket_core::GrantScope::Once,
        receipt: prov::ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: prov::InputProof::HardwareSeat,
            at: prov::UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    })]);
    let outcome = desk
        .companion
        .perform(propose("Sam prefers mornings"), Some(front.session), None)
        .await
        .expect("the request")
        .expect("it runs once the person says yes");
    assert_eq!(
        outcome.said.as_ref().map(LabelText::as_str),
        Some("Saved for you to keep or drop")
    );
    assert_eq!(outcome.undo, Undoable::No);
    let asked = memoryd.seen();
    let Some(MemoryRequest::Propose(in_space, draft)) = asked
        .iter()
        .find(|r| matches!(r, MemoryRequest::Propose(..)))
    else {
        panic!("{asked:?}")
    };
    assert_eq!(*in_space, space("work"));
    assert_eq!(draft.text.as_str(), "Sam prefers mornings");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_companion_starts_a_task_and_the_task_is_audited_into_memory() {
    let desk = Desk::start(FakeMemoryd::recording()).await;
    let front = desk.front().await;

    let outcome = desk
        .companion
        .perform(
            call(
                "org.quire.Companion",
                "companion.task.start",
                TargetValue::Nothing,
                &[("goal", Value::Text("find the Lisbon receipts".into()))],
            ),
            Some(front.session),
            None,
        )
        .await
        .expect("the request")
        .expect("the task starts");
    let Some(Labelled {
        value: Value::Text(task),
        ..
    }) = outcome.value
    else {
        panic!("{outcome:?}")
    };

    let worker = prov::TaskId::parse(&task).expect("a task id");
    let inbox = desk
        .companion
        .inbox(InboxAsk {
            agent: AgentRef::Worker {
                task: worker.clone(),
            },
            after: None,
        })
        .await
        .expect("inbox");
    assert_eq!(inbox.len(), 1, "the goal reached the worker as a request");

    let started = desk.records("docket.task_started").await;
    assert_eq!(started.len(), 1, "the new task is in the log");
    let messages = desk.records("companion.message").await;
    assert_eq!(messages.len(), 1, "and so is the request it was given");
    assert!(
        !desk.records("docket.call").await.is_empty(),
        "the call that started it too"
    );
}

fn first<T>(
    stream: &mut (impl futures_util::Stream<Item = T> + Unpin),
) -> impl std::future::Future<Output = T> + '_ {
    use futures_util::StreamExt;
    async move {
        tokio::time::timeout(Duration::from_secs(5), stream.next())
            .await
            .expect("the signal comes")
            .expect("a signal")
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_installed_manifest_appearing_changing_and_going_is_said_on_the_registry() {
    let desk = Desk::start(FakeMemoryd::recording()).await;
    let listener = desk.bus.connect().await;
    let registry = docket_dbus::RegistryProxy::new(&listener)
        .await
        .expect("proxy");
    let mut changed = registry
        .receive_manifest_changed()
        .await
        .expect("subscribed");
    let folder = desk.dir.path().join("data/quire/intents");
    std::fs::create_dir_all(&folder).expect("dir");

    std::fs::write(
        folder.join("org.quire.Mail.toml"),
        docket_fake::MAIL_MANIFEST,
    )
    .expect("write");
    let signal = first(&mut changed).await;
    assert_eq!(signal.args().expect("args").app(), &"org.quire.Mail");
    let listed = desk.companion.manifests().await.expect("manifests");
    assert!(
        listed
            .iter()
            .any(|m| m.manifest().app.as_str() == "org.quire.Mail")
    );

    let edited = docket_fake::MAIL_MANIFEST.replace("Read", "Look at");
    std::fs::write(folder.join("org.quire.Mail.toml"), edited).expect("write");
    assert_eq!(
        first(&mut changed).await.args().expect("args").app(),
        &"org.quire.Mail"
    );

    std::fs::remove_file(folder.join("org.quire.Mail.toml")).expect("remove");
    assert_eq!(
        first(&mut changed).await.args().expect("args").app(),
        &"org.quire.Mail"
    );
    let listed = desk.companion.manifests().await.expect("manifests");
    assert!(
        listed
            .iter()
            .all(|m| m.manifest().app.as_str() != "org.quire.Mail")
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_cannot_declare_the_built_in_providers() {
    let desk = Desk::start(FakeMemoryd::recording()).await;
    let folder = desk.dir.path().join("data/quire/intents");
    std::fs::create_dir_all(&folder).expect("dir");
    // A file that renames the memory actions' effects: the built-in declaration stands.
    let memory = include_str!("../../../manifests/org.quire.Memory.toml")
        .replace("reach = \"hidden\"", "reach = \"offered\"");
    std::fs::write(folder.join("org.quire.Memory.toml"), memory).expect("write");
    tokio::time::sleep(Duration::from_millis(400)).await;
    let listed = desk.companion.manifests().await.expect("manifests");
    let memory = listed
        .iter()
        .find(|m| m.manifest().app.as_str() == "org.quire.Memory")
        .expect("memory is still there");
    let forget = memory
        .manifest()
        .actions
        .iter()
        .find(|a| a.name.as_str() == "memory.forget")
        .expect("forget");
    assert_eq!(
        forget.reach,
        AgentReach::Hidden,
        "an agent never sees forgetting"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_message_arriving_for_a_worker_is_said_content_free() {
    let desk = Desk::start(FakeMemoryd::recording()).await;
    let listener = desk.bus.connect().await;
    let messages = docket_dbus::MessageProxy::new(&listener)
        .await
        .expect("proxy");
    let mut arrived = messages.receive_arrived().await.expect("subscribed");
    let front = desk.front().await;
    let outcome = desk
        .companion
        .perform(
            call(
                "org.quire.Companion",
                "companion.task.start",
                TargetValue::Nothing,
                &[("goal", Value::Text("find the receipts".into()))],
            ),
            Some(front.session),
            None,
        )
        .await
        .expect("the request")
        .expect("the task starts");
    let Some(Labelled {
        value: Value::Text(task),
        ..
    }) = outcome.value
    else {
        panic!("{outcome:?}")
    };
    let signal = first(&mut arrived).await;
    let agent: AgentRef =
        serde_json::from_str(signal.args().expect("args").agent()).expect("an agent");
    assert_eq!(
        agent,
        AgentRef::Worker {
            task: prov::TaskId::parse(&task).expect("task")
        }
    );
    assert!(
        !signal.args().expect("args").agent().contains("receipts"),
        "no content"
    );
}
