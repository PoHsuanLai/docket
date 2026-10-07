//! `Session.Read` through the running daemon: a companion recalls untrusted text (a handle),
//! asks the quarantined reader about it, and intentd carries the ask to readerd over `Reader1`
//! (the router names the session, readerd resolves the handle through `Session.Resolve` as the
//! reader) whose model is a scripted inferd. Everything is on a private bus.

use crate::support::bus::PrivateBus;
use crate::support::inferd::ScriptedInferd;
use crate::support::memoryd::FakeMemoryd;
use almanac_core::{
    InjectQuery, MemoryItem, MemoryReply, MemoryRequest, RecallHit, RecallOver, RecallWhy,
    TrustFilter, UserText,
};
use docket_client::{DbusTransport, Intents};
use docket_core::*;
use intentd::{Cadence, FileGrants, IntentdConfig, Setup, start};
use porter_core::{AppName, Count, DataClass, Tokens};
use prov::{AgentRef, Label, Source, SpaceId};
use readerd::{ReaderHost, ReaderService, serve_on};
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration;

const COMPANIOND: &str = "org.quire.Companiond";

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn config() -> IntentdConfig {
    IntentdConfig {
        roles: BTreeMap::from([
            (CallerRole::Companion, vec![app(COMPANIOND)]),
            (CallerRole::Reader, vec![app(docket_dbus::READER_BUS)]),
        ]),
        reviewers: None,
        agent: AgentConfig::default(),
    }
}

fn receipt_hit() -> RecallHit {
    RecallHit {
        doc: MemoryItem::Fact(
            almanac_core::FactId::parse("0123456789abcdefghjkmnpqrs").expect("fact id"),
        ),
        text: "Your hotel receipt".into(),
        at: prov::UnixSeconds(5),
        label: Label::untrusted(Source::Mail, DataClass::Mail, space()),
        links: vec![],
        why: RecallWhy::Lexical { rank: 1 },
    }
}

struct Desk {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    companion: Intents<DbusTransport>,
    inferd: ScriptedInferd,
    _connections: Vec<docket_dbus::BusConnection>,
    _intentd: intentd::Running,
}

impl Desk {
    /// intentd over a fake memoryd that holds one untrusted hit; readerd is started by the test.
    async fn start() -> Desk {
        let dir = tempfile::tempdir().expect("scratch");
        let bus = PrivateBus::start(dir.path());
        let memoryd = FakeMemoryd::with(|request| match request {
            MemoryRequest::Inject(_) => Some(MemoryReply::Hits(vec![receipt_hit()])),
            _ => None,
        });
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
        let _ = FileGrants::at(setup.grants.clone());
        setup.config = config();
        setup.signals = Cadence {
            every: Duration::from_millis(30),
            rescan_every: 2,
        };
        let daemon_connection = bus.connect().await;
        let intentd = start(&daemon_connection, None, setup)
            .await
            .expect("intentd");
        let companion_connection = bus.connect().await;
        companion_connection
            .request_name(COMPANIOND)
            .await
            .expect("name");
        Desk {
            _dir: dir,
            bus,
            companion: Intents::over(DbusTransport::new(companion_connection.clone())),
            inferd: ScriptedInferd::away(),
            _connections: vec![memory_connection, daemon_connection, companion_connection],
            _intentd: intentd,
        }
    }

    /// readerd on the bus, its model answering `answer`.
    async fn with_reader(mut self, answer: &str) -> Desk {
        self.inferd = ScriptedInferd::answering(answer);
        let connection = self.bus.connect().await;
        // readerd's own Intents1 caller is the connection that is about to own its name.
        let service = ReaderService::new(
            ReaderHost::start(),
            self.inferd.clone(),
            Intents::over(DbusTransport::new(connection.clone())),
        );
        serve_on(&connection, Arc::new(service))
            .await
            .expect("readerd serves");
        self._connections.push(connection);
        self
    }

    /// A front session and a handle to the untrusted receipt recalled into it.
    async fn session_and_handle(&self) -> (prov::SessionId, Handle) {
        let opened = self
            .companion
            .session_open(SessionOpen {
                space: space(),
                agent: AgentRef::Companion,
                parent: None,
            })
            .await
            .expect("a session");
        let inject = InjectQuery {
            space: space(),
            text: UserText::new("receipt".to_owned()),
            budget: Tokens(500),
            k: Count(3),
            over: RecallOver::Both,
            trust: TrustFilter::Any,
        };
        let RecallView::Hits(hits) = self
            .companion
            .session_recall(opened.session.clone(), RecallAsk::Inject(inject))
            .await
            .expect("recall")
        else {
            panic!("hits")
        };
        let Reveal::Handle(handle) = hits[0].text else {
            panic!("untrusted text is a handle")
        };
        (opened.session, handle)
    }
}

fn classify(handle: Handle) -> ReadAsk {
    ReadAsk {
        ask: ReaderAsk {
            inputs: vec![handle],
            want: ValueSchema::Choice(vec![
                ChoiceId::parse("receipt").expect("c"),
                ChoiceId::parse("newsletter").expect("c"),
            ]),
            task: ReaderTask::Classify,
        },
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn session_read_works_in_the_running_daemon_through_readerd_and_inferd() {
    let desk = Desk::start().await.with_reader("receipt").await;
    let (session, handle) = desk.session_and_handle().await;

    let answer = desk
        .companion
        .session_read(session, classify(handle))
        .await
        .expect("the reader answered");

    assert_eq!(
        answer,
        Reveal::Plain(Value::Choice(ChoiceId::parse("receipt").expect("c"))),
        "a closed-set answer is plain"
    );
    let opened = desk.inferd.opened();
    assert_eq!(opened.len(), 1, "readerd asked inferd once");
    // Session.Resolve answers the handle's own label too, so the read is sent as what the text is
    // (mail), not as the person's own words.
    assert_eq!(opened[0].class, DataClass::Mail);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_answer_outside_the_schema_is_refused_by_the_router_not_passed_on() {
    let desk = Desk::start().await.with_reader("invoice").await;
    let (session, handle) = desk.session_and_handle().await;
    let answer = desk.companion.session_read(session, classify(handle)).await;
    assert!(answer.is_err(), "{answer:?}");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_no_readerd_on_the_bus_the_read_is_refused() {
    let desk = Desk::start().await;
    let (session, handle) = desk.session_and_handle().await;
    let answer = desk.companion.session_read(session, classify(handle)).await;
    assert!(answer.is_err(), "{answer:?}");
}
