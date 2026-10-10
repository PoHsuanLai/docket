//! Per-call effects over a real bus: the router's `Classify` and `Perform` reach a provider
//! through intentd's `DbusLink` and the real `docket_client::serve_on`, with docket-fake's menu
//! app behind it. The seams that ask a person or a model are scripted; nothing else is.
//!
//! Covered: a Read item is not asked, a Destructive one is, a provider that classifies above its
//! ceiling is clamped, a failed `Classify` means the ceiling, a `Classified` that no longer
//! holds at `Perform` is asked again at the ceiling, and delegation (one ask in total for an
//! inner Destructive action, none for an inner Read one).

use crate::support::apps::Blank;
use crate::support::bus::PrivateBus;
use docket_client::{IntentProvider, serve_on};
use docket_core::*;
use docket_dbus::INTENTS_BUS;
use docket_fake::{
    FakeMemory, FakeMenu, FixedClock, MemoryGrants, MenuItem, RecordingSink, ScriptedConfirmer,
    ScriptedReader, ScriptedReviewer, ScriptedWriter,
};
use docket_router::{Router, Seams};
use intentd::DbusLink;
use policy_point::Pdp;
use porter_core::{AppId, AppName, Isolation};
use prov::{ActionName, AgentRef, Effect, SpaceId, UnixSeconds};
use std::collections::BTreeSet;
use std::sync::Arc;

/// The menu app, shared so a test can script it after it serves.
#[derive(Clone)]
struct SharedMenu(Arc<FakeMenu>);

impl IntentProvider for SharedMenu {
    fn manifest(&self) -> &ValidManifest {
        self.0.manifest()
    }
    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.0.perform(inv).await
    }
    async fn perform_classified(
        &self,
        inv: Invocation,
        activation: Option<ActivationToken>,
        classified: Option<CallClass>,
    ) -> Result<Outcome, AppRefusal> {
        self.0.perform_classified(inv, activation, classified).await
    }
    async fn classify(&self, inv: Invocation) -> Result<CallClass, AppRefusal> {
        self.0.classify(inv).await
    }
    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        self.0.dry_run(inv).await
    }
    async fn undo(&self, token: UndoToken, actor: prov::Actor) -> Result<(), UndoFault> {
        self.0.undo(token, actor).await
    }
    async fn search(&self, text: &str) -> Vec<Hit> {
        self.0.search(text).await
    }
    async fn preview(&self, id: &prov::EntityId) -> Preview {
        self.0.preview(id).await
    }
    async fn suggest(&self, ask: SuggestAsk) -> Vec<EntityRef> {
        self.0.suggest(ask).await
    }
}

/// Every fake, except that apps are reached over the bus.
struct BusSeams {
    link: DbusLink,
    confirmer: ScriptedConfirmer,
    reviewer: ScriptedReviewer,
    grants: MemoryGrants,
    sink: RecordingSink,
    clock: FixedClock,
    memory: FakeMemory,
    writer: ScriptedWriter,
    reader: ScriptedReader,
}

impl Seams for BusSeams {
    type Link = DbusLink;
    type Confirm = ScriptedConfirmer;
    type Review = ScriptedReviewer;
    type Grants = MemoryGrants;
    type Sink = RecordingSink;
    type Time = FixedClock;
    type Memory = FakeMemory;
    type Writer = ScriptedWriter;
    type Reading = ScriptedReader;
    type Log = docket_router::NoLog;
    type Checkpoints = docket_router::NoStore;
    fn link(&self) -> &DbusLink {
        &self.link
    }
    fn confirmer(&self) -> &ScriptedConfirmer {
        &self.confirmer
    }
    fn reviewer(&self) -> &ScriptedReviewer {
        &self.reviewer
    }
    fn grants(&self) -> &MemoryGrants {
        &self.grants
    }
    fn sink(&self) -> &RecordingSink {
        &self.sink
    }
    fn clock(&self) -> &FixedClock {
        &self.clock
    }
    fn memory(&self) -> &FakeMemory {
        &self.memory
    }
    fn writer(&self) -> &ScriptedWriter {
        &self.writer
    }
    fn reader(&self) -> &ScriptedReader {
        &self.reader
    }
    fn log(&self) -> &docket_router::NoLog {
        &docket_router::NoLog
    }
    fn checkpoints(&self) -> &docket_router::NoStore {
        &docket_router::NoStore
    }
}

struct Desk {
    _dir: tempfile::TempDir,
    _bus: PrivateBus,
    _app: docket_dbus::BusConnection,
    router: Router<BusSeams>,
    menu: SharedMenu,
}

fn menu_app() -> AppName {
    AppName::parse("org.quire.Menu").expect("app")
}

fn companion() -> CallerId {
    CallerId {
        app: AppId {
            name: AppName::parse("org.quire.Companiond").expect("app"),
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([CallerRole::Companion]),
    }
}

/// A router over a private bus with the menu app serving on it, and a session ready.
async fn desk(answers: usize) -> Desk {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    daemon
        .request_name(INTENTS_BUS)
        .await
        .expect("intentd's name");
    let app = bus.connect().await;
    let manifest = docket_fake::menu_manifest().expect("manifest");
    let menu = SharedMenu(Arc::new(FakeMenu::new(manifest.clone())));
    serve_on(&app, menu.clone(), Blank(menu_app()), Blank(menu_app()))
        .await
        .expect("the menu serves");
    let receipt = prov::ConfirmReceipt {
        id: prov::ConfirmId::parse("c-1").expect("id"),
        input: prov::InputProof::HardwareSeat,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    };
    let yes = ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt,
    };
    let seams = BusSeams {
        link: DbusLink::new(daemon),
        confirmer: ScriptedConfirmer::answering((0..answers).map(|_| yes.clone()).collect()),
        reviewer: ScriptedReviewer::always_allow(),
        grants: MemoryGrants::new(),
        sink: RecordingSink::new(),
        clock: FixedClock::at(UnixSeconds(0)),
        memory: FakeMemory::default(),
        writer: ScriptedWriter::failing(),
        reader: ScriptedReader::default(),
    };
    {
        use docket_router::GrantStore;
        use porter_core::consent::{Decision, Grant, GrantScope, Usage};
        for (n, usage) in [Usage::Interactive, Usage::Background]
            .into_iter()
            .enumerate()
        {
            seams.grants.record(Grant {
                id: porter_core::GrantId::parse(&format!("g-{n}")).expect("grant"),
                key: ActionGrantKey {
                    caller: GrantCaller::Companion,
                    owner: menu_app(),
                    target: GrantTarget::App,
                    class: porter_core::DataClass::Files,
                    usage,
                    space: prov::SpaceScope::Only(SpaceId::parse("work").expect("space")),
                },
                decision: Decision::Allow,
                scope: GrantScope::Always,
                at: UnixSeconds(0),
            });
        }
    }
    let router = Router::new(
        seams,
        AgentConfig::default(),
        Pdp::standard().expect("policy"),
    );
    router.state.lock().expect("lock").registry.insert(manifest);
    let opened = router
        .handle(
            &companion(),
            IntentsRequest::SessionOpen(SessionOpen {
                space: SpaceId::parse("work").expect("space"),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
            }),
        )
        .await;
    assert!(
        matches!(opened, IntentsReply::SessionOpened(_)),
        "{opened:?}"
    );
    Desk {
        _dir: dir,
        _bus: bus,
        _app: app,
        router,
        menu,
    }
}

fn call(name: &str, item: &str) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: menu_app(),
            name: ActionName::parse(name).expect("action"),
        },
        target: TargetValue::Nothing,
        args: std::iter::once((
            ParamName::parse("item").expect("param"),
            prov::Labelled {
                value: Value::Text(item.into()),
                label: prov::Label::trusted_user(),
            },
        ))
        .collect(),
        origin: Origin::Companion,
    }
}

async fn run(desk: &Desk, request: CallRequest) -> Result<Outcome, CallRefusal> {
    let reply = desk
        .router
        .handle(
            &companion(),
            IntentsRequest::Perform {
                activation: None,
                call: request,
                session: None,
                parent_window: None,
            },
        )
        .await;
    match reply {
        IntentsReply::Performed(result) => *result,
        other => panic!("perform: {other:?}"),
    }
}

fn asks(desk: &Desk) -> usize {
    desk.router.seams.confirmer.requests().len()
}

fn classified(desk: &Desk) -> Vec<Classification> {
    desk.router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Classified { classification, .. } => Some(classification),
            _ => None,
        })
        .collect()
}

fn effects(desk: &Desk) -> Vec<(String, Effect)> {
    desk.router
        .seams
        .sink
        .records()
        .into_iter()
        .filter_map(|r| match r {
            AuditRecord::Call { action, effect, .. } => {
                Some((action.name.as_str().to_owned(), effect))
            }
            _ => None,
        })
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_read_item_is_not_asked_and_a_destructive_one_is() {
    let desk = desk(1).await;
    desk.menu
        .0
        .script("Minimise", MenuItem::effect(Effect::Read));
    desk.menu
        .0
        .script("Delete", MenuItem::effect(Effect::Destructive));
    run(&desk, call("menu.item.activate", "Minimise"))
        .await
        .expect("runs unasked");
    assert_eq!(asks(&desk), 0);
    run(&desk, call("menu.item.activate", "Delete"))
        .await
        .expect("runs once allowed");
    assert_eq!(asks(&desk), 1);
    let c = classified(&desk);
    assert_eq!(
        (c[0].ceiling, c[0].used),
        (Effect::Destructive, Effect::Read)
    );
    assert_eq!(
        (c[1].ceiling, c[1].used),
        (Effect::Destructive, Effect::Destructive)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lying_provider_is_clamped_and_a_failed_classify_means_the_ceiling() {
    let desk = desk(1).await;
    // Declared an undoable write; claims destructive. Clamped.
    desk.menu
        .0
        .script("Adjust", MenuItem::effect(Effect::Destructive));
    run(&desk, call("menu.item.adjust", "Adjust")).await.ok();
    let c = classified(&desk);
    assert_eq!(c[0].used, Effect::UndoableWrite);
    assert_eq!(c[0].sent, CallClass::Effect(Effect::UndoableWrite));
    // An item the provider refuses to classify (it does not know it): the ceiling, an ask.
    run(&desk, call("menu.item.activate", "Unknown")).await.ok();
    let c = classified(&desk);
    assert_eq!(
        c[1].answer,
        ClassifyAnswer::Failed(ClassifyFault::Refused),
        "{c:?}"
    );
    assert_eq!(c[1].used, Effect::Destructive);
    assert_eq!(asks(&desk), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_classification_that_changed_is_asked_again_at_the_ceiling() {
    let desk = desk(1).await;
    desk.menu.0.script(
        "Shifty",
        MenuItem {
            classify: Ok(CallClass::Effect(Effect::Read)),
            at_perform: Some(CallClass::Effect(Effect::Destructive)),
            perform: docket_fake::MenuPerform::Done,
        },
    );
    run(&desk, call("menu.item.activate", "Shifty"))
        .await
        .expect("runs after the ask");
    assert_eq!(asks(&desk), 1);
    let c = classified(&desk);
    assert_eq!((c[0].used, c[1].used), (Effect::Read, Effect::Destructive));
    assert_eq!(desk.menu.0.classified().len(), 1, "never classified twice");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn delegation_asks_once_for_a_destructive_inner_action_and_never_for_a_read_one() {
    let desk = desk(2).await;
    desk.menu
        .0
        .script("Hide", MenuItem::delegating("menu.window.hide"));
    desk.menu
        .0
        .script("Close", MenuItem::delegating("menu.window.close"));
    run(&desk, call("menu.item.activate", "Hide"))
        .await
        .expect("runs unasked");
    assert_eq!(asks(&desk), 0, "an inner Read action asks nobody");
    run(&desk, call("menu.item.activate", "Close"))
        .await
        .expect("runs once allowed");
    assert_eq!(asks(&desk), 1, "one ask in total, for the inner action");
    assert_eq!(
        effects(&desk),
        vec![
            ("menu.item.activate".to_owned(), Effect::Read),
            ("menu.window.hide".to_owned(), Effect::Read),
            ("menu.item.activate".to_owned(), Effect::Read),
            ("menu.window.close".to_owned(), Effect::Destructive),
        ]
    );
}
