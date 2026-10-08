//! Every member of `org.quire.Intents1` over a real bus: intentd's `serve_on` on one end, the
//! client's `DbusTransport` on the other, and a second router, in process, given the same
//! requests as the same caller. Whatever the transport, the reply must be the router's own, so
//! the codec of each member (the JSON of its arguments, the Request objects of the members that
//! wait, the bus errors of a request refused before it was a call) is held to the router
//! instead of to a second opinion of what it should say.
//!
//! Also: the introspection of what intentd serves is `dbus/org.quire.Intents1.xml`, and the
//! caller's identity is derived from the connection, never sent.

use crate::support::world::*;
use almanac_core::{
    BodyMode, Episode, EpisodeId, EpisodeKind, EpisodeOutcome, RecentQuery, Skeleton, TrustFilter,
};
use docket_client::{Transport, TransportError};
use docket_core::*;
use docket_dbus::{Bus, introspection};
use porter_core::Count;
use prov::{
    ActionName, Address, AgentRef, Effect, EntityId, EntityKey, EntityKind, Label, MessageKind,
    SessionId, SpaceId, SpaceScope, UnixSeconds,
};
use std::collections::BTreeSet;

fn space(name: &str) -> SpaceId {
    SpaceId::parse(name).expect("space")
}

fn mail() -> porter_core::AppName {
    app("org.quire.Mail")
}

fn thread(key: &str) -> EntityId {
    EntityId {
        app: mail(),
        kind: EntityKind::parse("mail.thread").expect("kind"),
        key: EntityKey::parse(key).expect("key"),
    }
}

fn act(name: &str) -> ActionRef {
    ActionRef {
        app: mail(),
        name: ActionName::parse(name).expect("action"),
    }
}

fn call(name: &str, key: &str) -> CallRequest {
    CallRequest {
        action: act(name),
        target: TargetValue::Entities(vec![thread(key)]),
        args: Args::new(),
        origin: Origin::Launcher,
    }
}

fn cua_ask() -> CuaAsk {
    CuaAsk {
        run: prov::RunId::parse("r-1").expect("run"),
        step: 1,
        app: mail(),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: space("work"),
        action: cua_action::CuaAction::<cua_action::WindowSpace>::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: Label::trusted_user(),
    }
}

fn episode() -> Episode {
    Episode {
        id: EpisodeId::parse("t-1").expect("episode"),
        agent: AgentRef::Companion,
        kind: EpisodeKind::Task,
        parent: None,
        space: space("work"),
        started: UnixSeconds(0),
        ended: UnixSeconds(1),
        outcome: EpisodeOutcome::Done,
        skeleton: Skeleton {
            label: Label::trusted_user(),
            asked: vec![],
            steps: vec![],
            touched: vec![],
            results: vec![],
        },
        narrative: None,
    }
}

/// Two routers that must answer alike: the one on the bus and the one in the test.
struct Pair {
    transport: docket_client::DbusTransport,
    local: docket_router::Router<docket_fake::FakeSeams>,
    caller: CallerId,
    covered: BTreeSet<Member>,
}

impl Pair {
    /// Sends `request` both ways and asserts the replies are one.
    async fn both(&mut self, request: IntentsRequest) -> IntentsReply {
        self.covered.insert(request.member());
        let over_bus = self
            .transport
            .call(request.clone())
            .await
            .unwrap_or_else(|e| panic!("{:?}: {e}", request.member()));
        let direct = self.local.handle(&self.caller, request.clone()).await;
        assert_eq!(
            over_bus,
            direct,
            "{:?} over the bus is not the router's own",
            request.member()
        );
        over_bus
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn every_member_over_the_bus_answers_what_the_router_answers() {
    let world = World::serving(mail_router(), everything_config()).await;
    let (everything, _keep) = world.client(&[EVERYTHING]).await;
    let all_roles: Vec<CallerRole> = CallerRole::ALL
        .iter()
        .copied()
        .filter(|r| *r != CallerRole::App)
        .collect();
    let mut pair = Pair {
        transport: everything,
        local: mail_router(),
        caller: caller(EVERYTHING, &all_roles),
        covered: BTreeSet::new(),
    };
    let (mail_transport, _mail_keep) = world.client(&["org.quire.Mail"]).await;
    let local_mail = caller("org.quire.Mail", &[]);

    // The registry: the installed manifests.
    let IntentsReply::Manifests(manifests) = pair.both(IntentsRequest::Manifests).await else {
        panic!("manifests");
    };
    assert!(
        manifests.len() >= 4,
        "the fixtures and the built-ins: {}",
        manifests.len()
    );

    // The index is the owning app's, so another connection pushes it.
    let batch = IndexBatch {
        epoch: 1,
        upserts: vec![IndexEntry {
            key: EntityKey::parse("t1").expect("key"),
            kind: EntityKind::parse("mail.thread").expect("kind"),
            title: "Invoice".into(),
            subtitle: "from the world".into(),
            keywords: vec![],
            updated: UnixSeconds(1),
            space: SpaceScope::Only(space("work")),
        }],
        removals: vec![],
    };
    for request in [
        IntentsRequest::IndexReset { epoch: 1 },
        IntentsRequest::IndexPush(batch),
    ] {
        let over_bus = mail_transport.call(request.clone()).await.expect("index");
        let direct = pair.local.handle(&local_mail, request).await;
        assert_eq!(over_bus, direct);
        assert_eq!(over_bus, IntentsReply::Done);
        pair.covered
            .insert(IntentsRequest::IndexReset { epoch: 1 }.member());
        pair.covered.insert(
            IntentsRequest::IndexPush(IndexBatch {
                epoch: 1,
                upserts: vec![],
                removals: vec![],
            })
            .member(),
        );
    }
    // Search and its cancel.
    let ask = SearchAsk {
        text: "invoice".into(),
        scope: SearchScope::Everything,
        generation: Generation(1),
    };
    pair.both(IntentsRequest::Search(ask)).await;
    pair.both(IntentsRequest::SearchCancel(Generation(1))).await;

    // A session, a turn, the task policy, a refused widening, context, handles, reading.
    let open = SessionOpen {
        space: space("work"),
        agent: AgentRef::Companion,
        parent: None,
        cwd: None,
        started_from: None,
        external: None,
    };
    let IntentsReply::SessionOpened(opened) = pair.both(IntentsRequest::SessionOpen(open)).await
    else {
        panic!("session");
    };
    let session = opened.session;
    let turn = TurnIn {
        text: "tidy my inbox".into(),
        origin: Origin::Launcher,
        keep: ContextKeep {
            query: Keep::Dropped,
            results: Keep::Dropped,
            selection: Keep::Dropped,
            window: Keep::Dropped,
        },
        via: TurnVia::Typed,
    };
    let IntentsReply::TurnRecorded(turn_id) = pair
        .both(IntentsRequest::SessionTurn {
            session: session.clone(),
            turn,
        })
        .await
    else {
        panic!("turn");
    };
    pair.both(IntentsRequest::SessionTaskPolicy {
        session: session.clone(),
    })
    .await;
    let widen = pair
        .both(IntentsRequest::SessionWiden {
            session: session.clone(),
            widen: WidenAsk {
                turn: TurnId(turn_id.0 + 1000),
                change: sample_policy(&opened.task),
            },
        })
        .await;
    assert_eq!(
        widen,
        IntentsReply::Refused(WireRefusal::Malformed),
        "a turn the person never said"
    );
    pair.both(IntentsRequest::Context {
        session: session.clone(),
        app: mail(),
    })
    .await;
    pair.both(IntentsRequest::SessionResolve {
        session: session.clone(),
        handle: Handle(1),
    })
    .await;
    pair.both(IntentsRequest::SessionDisplay {
        session: session.clone(),
        handle: Handle(1),
    })
    .await;
    pair.both(IntentsRequest::SessionRead {
        session: session.clone(),
        ask: ReadAsk {
            ask: ReaderAsk {
                inputs: vec![Handle(1)],
                want: ValueSchema::Integer { min: 0, max: 9 },
                task: ReaderTask::Classify,
            },
        },
    })
    .await;
    pair.both(IntentsRequest::SessionNote {
        session: session.clone(),
        note: NoteAsk::Episode(Box::new(episode())),
    })
    .await;
    pair.both(IntentsRequest::SessionNote {
        session: session.clone(),
        note: NoteAsk::Record(SessionNote {
            slug: NoteSlug::parse("opened").expect("slug"),
            json: almanac_core::JsonText::parse(r#"{"kind":"closed"}"#).expect("json"),
        }),
    })
    .await;
    pair.both(IntentsRequest::SessionHandles {
        session: session.clone(),
    })
    .await;
    pair.both(IntentsRequest::SessionStored {
        ask: docket_core::StoredAsk::List,
    })
    .await;
    pair.both(IntentsRequest::SessionNarrow {
        session: session.clone(),
        turn: UserTurn {
            id: TurnId(1),
            text: "only read it".into(),
            at: UnixSeconds(1),
            from: TurnSource::Launcher,
            via: TurnVia::Typed,
        },
    })
    .await;
    for ask in [RecallAsk::Primer, RecallAsk::Profile, RecallAsk::Spaces] {
        pair.both(IntentsRequest::SessionRecall {
            session: session.clone(),
            ask,
        })
        .await;
    }
    pair.both(IntentsRequest::SessionRecall {
        session: session.clone(),
        ask: almanac_core_recall(),
    })
    .await;

    // Messages: a note to the person, and the person's inbox.
    let draft = MessageDraft {
        to: Address {
            agent: AgentRef::User,
            space: space("work"),
        },
        thread: None,
        in_reply_to: None,
        kind: MessageKind::Note,
        parts: vec![DraftPart::Text(prov::MessageText::new("all done"))],
    };
    pair.both(IntentsRequest::MessageSend {
        session: session.clone(),
        draft,
    })
    .await;
    pair.both(IntentsRequest::MessageInbox(InboxAsk {
        agent: AgentRef::User,
        after: None,
    }))
    .await;

    // Calls: a read, a dry run, a write, a refusal that is an answer, and the journal's undo.
    let read = pair
        .both(IntentsRequest::Perform {
            activation: None,
            call: call("mail.thread.read", "t2"),
            session: None,
            parent_window: None,
        })
        .await;
    assert!(
        matches!(read, IntentsReply::Performed(ref end) if end.is_ok()),
        "{read:?}"
    );
    let dry = pair
        .both(IntentsRequest::DryRun {
            call: call("mail.thread.archive", "t2"),
            session: None,
        })
        .await;
    assert!(matches!(dry, IntentsReply::Preview(_)), "{dry:?}");
    let archived = pair
        .both(IntentsRequest::Perform {
            activation: None,
            call: call("mail.thread.archive", "t2"),
            session: None,
            parent_window: None,
        })
        .await;
    assert!(
        matches!(archived, IntentsReply::Performed(_)),
        "{archived:?}"
    );
    let missing = pair
        .both(IntentsRequest::Perform {
            activation: None,
            call: call("mail.thread.nonesuch", "t2"),
            session: None,
            parent_window: None,
        })
        .await;
    assert!(
        matches!(missing, IntentsReply::Performed(ref end) if end.is_err()),
        "a refused call is an answer, not an error: {missing:?}"
    );
    let refused_dry = pair
        .both(IntentsRequest::DryRun {
            call: call("mail.thread.nonesuch", "t2"),
            session: None,
        })
        .await;
    assert!(
        matches!(refused_dry, IntentsReply::Refused(WireRefusal::Call(_))),
        "{refused_dry:?}"
    );
    pair.both(IntentsRequest::Preview(thread("t2"))).await;
    pair.both(IntentsRequest::Suggest(SuggestAsk {
        action: act("mail.message.send"),
        param: ParamName::parse("to").expect("param"),
        typed: "acc".into(),
    }))
    .await;
    let IntentsReply::Journal(rows) = pair
        .both(IntentsRequest::ControlJournal(JournalFilter {
            run: None,
            session: None,
            limit: Count(10),
        }))
        .await
    else {
        panic!("journal");
    };
    let entry = rows.first().map_or(UndoId(1), |row| row.id);
    pair.both(IntentsRequest::Undo(entry)).await;
    pair.both(IntentsRequest::UndoAll(UndoScope::Entry(entry)))
        .await;

    // The computer-use gate.
    pair.both(IntentsRequest::GateGrant(GrantAsk {
        app: mail(),
        space: space("work"),
    }))
    .await;
    pair.both(IntentsRequest::GateCheck(cua_ask())).await;

    // The control centre: state, halt, resume, and the terminal's grants.
    pair.both(IntentsRequest::ControlState).await;
    pair.both(IntentsRequest::ControlHalt {
        scope: SpaceScope::Any,
        cause: HaltCause::ControlCentre,
    })
    .await;
    pair.both(IntentsRequest::ControlState).await;
    pair.both(IntentsRequest::ControlResume {
        scope: SpaceScope::Any,
    })
    .await;
    pair.both(IntentsRequest::ControlTerminalGrants).await;
    pair.both(IntentsRequest::ControlTerminalRevoke(act(
        "mail.thread.archive",
    )))
    .await;
    pair.both(IntentsRequest::ControlStandingGrants).await;
    pair.both(IntentsRequest::ControlStandingRevoke(
        docket_core::StandingGrantId::parse("sg-0000000000000000").expect("id"),
    ))
    .await;
    pair.both(IntentsRequest::SessionClose { session }).await;

    let missing: Vec<Member> = Member::ALL
        .iter()
        .copied()
        .filter(|m| !pair.covered.contains(m))
        .collect();
    assert!(
        missing.is_empty(),
        "members never called over the bus: {missing:?}"
    );
}

fn almanac_core_recall() -> RecallAsk {
    RecallAsk::Recent(RecentQuery {
        since: UnixSeconds(0),
        kinds: vec![],
        trust: TrustFilter::Any,
        limit: Count(5),
        bodies: BodyMode::Without,
    })
}

fn sample_policy(task: &prov::TaskId) -> TaskPolicy {
    TaskPolicy {
        task: task.clone(),
        space: space("work"),
        from: vec![],
        actions: BTreeSet::from([ActionMatch::AppUpTo(mail(), Effect::Destructive)]),
        kinds: BTreeSet::new(),
        ceiling: Effect::Destructive,
        max_count: Count(100),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("test policy").expect("words"),
        state: TaskPolicyState::Active,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_caller_with_no_role_is_a_plain_app_and_an_unnamed_one_is_nobody() {
    let world = World::serving(mail_router(), everything_config()).await;
    // A plain app may read the registry, and nothing the launcher alone may do.
    let (plain, _keep) = world.client(&["org.quire.Mail"]).await;
    let manifests = plain
        .call(IntentsRequest::Manifests)
        .await
        .expect("manifests");
    assert!(matches!(manifests, IntentsReply::Manifests(_)));
    let halt = plain
        .call(IntentsRequest::ControlHalt {
            scope: SpaceScope::Any,
            cause: HaltCause::ControlCentre,
        })
        .await
        .expect("a reply");
    assert_eq!(halt, IntentsReply::Refused(WireRefusal::NotAllowed));
    assert_eq!(
        world
            .router
            .handle(&caller("org.quire.Mail", &[]), IntentsRequest::ControlState)
            .await,
        IntentsReply::Refused(WireRefusal::NotAllowed)
    );
    // A connection that owns no name and runs in an unnamed service unit has no identity: nobody.
    let (nobody, _keep) = world.client(&[]).await;
    let reply = nobody
        .call(IntentsRequest::Manifests)
        .await
        .expect("a reply");
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_the_configuration_does_not_list_gets_no_roles_even_if_it_says_it_is_sill() {
    // The role comes from the name the connection owns, never from anything in a body.
    let world = World::serving(mail_router(), everything_config()).await;
    let (impostor, _keep) = world.client(&["org.quire.Shell"]).await;
    let reply = impostor
        .call(IntentsRequest::ControlHalt {
            scope: SpaceScope::Any,
            cause: HaltCause::ControlCentre,
        })
        .await
        .expect("a reply");
    assert_eq!(reply, IntentsReply::Refused(WireRefusal::NotAllowed));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_interfaces_intentd_serves_are_the_declared_ones() {
    let world = World::serving(mail_router(), everything_config()).await;
    let (client, connection) = world.client(&[EVERYTHING]).await;
    let _ = client;
    let proxy = zbus::fdo::IntrospectableProxy::builder(&connection)
        .destination(docket_dbus::INTENTS_BUS)
        .expect("destination")
        .path(docket_dbus::INTENTS_PATH)
        .expect("path")
        .build()
        .await
        .expect("proxy");
    let served = proxy.introspect().await.expect("introspection");
    // The bus adds the standard interfaces; what is ours is every `org.quire.Intents1.*`
    // interface but `Request`, which lives at the request objects.
    let declared = introspection(Bus::Intents);
    for interface in [
        "Registry", "Index", "Search", "Run", "Context", "Session", "Message", "Gate", "Control",
    ] {
        let name = format!("org.quire.Intents1.{interface}");
        let theirs = block(&declared, &name);
        let ours = block(&served, &name);
        assert_eq!(ours, theirs, "{name} is served as declared");
    }
}

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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_request_that_the_bus_cannot_carry_to_intentd_is_closed_not_a_hang() {
    let world = World::serving(mail_router(), everything_config()).await;
    let (everything, _keep) = world.client(&[EVERYTHING]).await;
    world.daemon().clone().close().await.expect("close");
    let reply = everything.call(IntentsRequest::ControlState).await;
    assert!(
        matches!(reply, Err(TransportError::Closed | TransportError::Bus(_))),
        "{reply:?}"
    );
}

#[allow(dead_code)]
fn session_id(text: &str) -> SessionId {
    SessionId::parse(text).expect("session")
}
