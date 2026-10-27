//! The apps over a real bus: `DbusLink` on intentd's side, the real `docket_client::serve_on` on
//! the app's, with docket-fake's mail app behind it. A provider answers nothing but intentd, an
//! app that is away is "unavailable" rather than a panic, and a slow one is cut at its latency.

use crate::support::apps::{Blank, answering_intentd, serve_mail};
use crate::support::bus::PrivateBus;
use crate::support::world::app;
use docket_client::{ContextSource, IntentProvider, SummonTarget, serve_on_with};
use docket_core::LinkFault;
use docket_core::*;
use docket_dbus::{INTENTS_BUS, IntentProviderProxy};
use docket_router::{AppFault, AppLink};
use intentd::DbusLink;
use prov::{Actor, EntityId, EntityKey, EntityKind, SpaceId};
use std::time::Duration;

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

fn invocation(action: &str, key: &str) -> Invocation {
    Invocation {
        call: CallId(1),
        action: prov::ActionName::parse(action).expect("action"),
        target: TargetValue::Entities(vec![thread(key)]),
        args: Args::new(),
        actor: Actor::Cli,
        origin: Origin::Cli,
        space: SpaceId::parse("work").expect("space"),
    }
}

/// Starts a bus where `daemon` owns intentd's name, as the daemon does.
async fn bus_with_intentd() -> (
    tempfile::TempDir,
    PrivateBus,
    docket_dbus::BusConnection,
    DbusLink,
) {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    daemon
        .request_name(INTENTS_BUS)
        .await
        .expect("intentd's name");
    let link = DbusLink::new(daemon.clone());
    (dir, bus, daemon, link)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_member_reaches_the_app_and_comes_back_typed() {
    let (dir, bus, daemon, link) = bus_with_intentd().await;
    let app_connection = bus.connect().await;
    let who = answering_intentd(&app_connection, &daemon, dir.path());
    let mail_app = serve_mail(&app_connection, who).await;

    // Perform, within the action's latency.
    let read = link
        .perform(
            &mail(),
            invocation("mail.thread.read", "t2"),
            Latency::Instant,
        )
        .await
        .expect("the app answers");
    assert_eq!(
        read.value.expect("the thread").value,
        Value::Text("This week".into())
    );
    let archived = link
        .perform(
            &mail(),
            invocation("mail.thread.archive", "t2"),
            Latency::Quick,
        )
        .await
        .expect("archived");
    assert!(mail_app.0.is_archived("t2"));
    let Undoable::Yes(token) = archived.undo else {
        panic!("an undoable write: {archived:?}");
    };

    // The app's own refusal is the app's, not a transport fault.
    let refused = link
        .perform(
            &mail(),
            invocation("mail.thread.read", "nope"),
            Latency::Instant,
        )
        .await;
    assert!(
        matches!(refused, Err(AppFault::Refused(AppRefusal::NotFound(_)))),
        "{refused:?}"
    );

    // The preview before the change, the undo after it.
    let preview = link
        .dry_run(&mail(), invocation("mail.thread.archive", "t1"))
        .await;
    assert!(preview.is_ok(), "{preview:?}");
    assert_eq!(link.undo(&mail(), &token, &Actor::Cli).await, Ok(()));
    assert!(!mail_app.0.is_archived("t2"), "the app undid it");
    assert_eq!(
        link.undo(&mail(), &token, &Actor::Cli).await,
        Err(UndoFault::Gone),
        "an undo the app no longer holds is gone"
    );

    // Context, preview and suggestions.
    let context = link
        .context(&mail(), ContextScope::ActiveWindow)
        .await
        .expect("a snapshot");
    assert_eq!(context.app, mail());
    assert!(
        link.preview(&mail(), &thread("t1")).await.is_ok(),
        "preview"
    );
    let ask = SuggestAsk {
        action: ActionRef {
            app: mail(),
            name: prov::ActionName::parse("mail.message.send").expect("action"),
        },
        param: ParamName::parse("to").expect("param"),
        typed: "acc".into(),
    };
    assert!(link.suggest(&mail(), ask).await.is_ok(), "suggest");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_app_that_is_away_is_unavailable_and_never_a_panic() {
    let (_dir, _bus, _daemon, link) = bus_with_intentd().await;
    let gone = app("org.quire.Gone");
    let performed = link
        .perform(&gone, invocation("gone.do", "x"), Latency::Instant)
        .await;
    assert!(performed == Err(AppFault::Unavailable), "{performed:?}");
    assert!(
        link.dry_run(&gone, invocation("gone.do", "x"))
            .await
            .is_err()
    );
    let token: UndoToken = serde_json::from_str("\"u-1\"").expect("a token");
    assert_eq!(
        link.undo(&gone, &token, &Actor::Cli).await,
        Err(UndoFault::AppUnavailable)
    );
    assert_eq!(
        link.context(&gone, ContextScope::ActiveWindow).await,
        Err(LinkFault::Unavailable)
    );
    assert_eq!(
        link.preview(&gone, &thread("x")).await,
        Err(LinkFault::Unavailable)
    );
}

/// An app that takes its time.
struct Slow(ValidManifest);

impl IntentProvider for Slow {
    fn manifest(&self) -> &ValidManifest {
        &self.0
    }
    async fn perform(&self, _: Invocation) -> Result<Outcome, AppRefusal> {
        tokio::time::sleep(Duration::from_secs(3)).await;
        Err(AppRefusal::Busy)
    }
    async fn dry_run(&self, _: Invocation) -> Result<Preview, AppRefusal> {
        Ok(Preview::None)
    }
    async fn undo(&self, _: UndoToken, _: Actor) -> Result<(), UndoFault> {
        Ok(())
    }
    async fn search(&self, _: &str) -> Vec<Hit> {
        vec![]
    }
    async fn preview(&self, _: &EntityId) -> Preview {
        Preview::None
    }
    async fn suggest(&self, _: SuggestAsk) -> Vec<EntityRef> {
        vec![]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_slow_app_is_cut_at_its_latency() {
    let (dir, bus, daemon, link) = bus_with_intentd().await;
    let slow_connection = bus.connect().await;
    let who = answering_intentd(&slow_connection, &daemon, dir.path());
    let manifest = validate(Manifest {
        vocab: IntentsVocab(1),
        app: app("org.quire.Slow"),
        entities: vec![],
        actions: vec![],
        visibility: Visibility::Everyone,
    })
    .expect("a manifest");
    serve_on_with(
        &slow_connection,
        Slow(manifest),
        Blank(app("org.quire.Slow")),
        Blank(app("org.quire.Slow")),
        who,
    )
    .await
    .expect("serves");
    let slow = app("org.quire.Slow");
    let started = std::time::Instant::now();
    let performed = link
        .perform(&slow, invocation("slow.do", "x"), Latency::Instant)
        .await;
    assert_eq!(performed, Err(AppFault::TimedOut));
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "cut at 250 ms, not left to finish: {:?}",
        started.elapsed()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_answers_nobody_but_intentd() {
    let (dir, bus, daemon, _link) = bus_with_intentd().await;
    let app_connection = bus.connect().await;
    let who = answering_intentd(&app_connection, &daemon, dir.path());
    serve_mail(&app_connection, who.clone()).await;
    // A terminal, another app, a scope that claims intentd's app, anything that is not the process
    // in intentd's unit calling the app's bus name directly gets no way around the gate.
    let stranger = bus.connect().await;
    let lookalike = bus.connect().await;
    who.introduce_app(
        &lookalike,
        "org.quire.Intents",
        docket_dbus::callers::Role::App,
    );
    let agent = bus.connect().await;
    who.introduce_app(
        &agent,
        "org.quire.Companion1",
        docket_dbus::callers::Role::Agent,
    );
    for caller in [&lookalike, &agent] {
        let proxy = IntentProviderProxy::builder(caller)
            .destination("org.quire.Mail")
            .expect("destination")
            .build()
            .await
            .expect("proxy");
        let direct = proxy
            .perform(
                &serde_json::to_string(&invocation("mail.thread.delete", "t1")).expect("json"),
                &docket_dbus::Details::new(),
            )
            .await;
        assert!(direct.is_err(), "{direct:?}");
    }
    let proxy = IntentProviderProxy::builder(&stranger)
        .destination("org.quire.Mail")
        .expect("destination")
        .build()
        .await
        .expect("proxy");
    let invocation = serde_json::to_string(&invocation("mail.thread.delete", "t1")).expect("json");
    let direct = proxy
        .perform(&invocation, &docket_dbus::Details::new())
        .await;
    let error = direct.expect_err("refused");
    assert!(
        error.to_string().contains("only intentd calls a provider"),
        "{error}"
    );
    for member in [
        proxy.search("x", 1).await,
        proxy.preview("{}").await,
        proxy.context("\"active_window\"").await,
    ] {
        assert!(member.is_err(), "{member:?}");
    }
}

/// `Blank` keeps `ContextSource` and `SummonTarget` in scope for the slow app above.
#[allow(dead_code)]
fn _seams<T: ContextSource + SummonTarget>() {}

/// An app that keeps the activation token it is handed (and refuses the call).
struct Keeps(
    ValidManifest,
    std::sync::Arc<std::sync::Mutex<Vec<Option<ActivationToken>>>>,
);

impl IntentProvider for Keeps {
    fn manifest(&self) -> &ValidManifest {
        &self.0
    }
    async fn perform(&self, _: Invocation) -> Result<Outcome, AppRefusal> {
        self.1.lock().expect("log").push(None);
        Err(AppRefusal::Busy)
    }
    async fn perform_activated(
        &self,
        _: Invocation,
        activation: Option<ActivationToken>,
    ) -> Result<Outcome, AppRefusal> {
        self.1.lock().expect("log").push(activation);
        Err(AppRefusal::Busy)
    }
    async fn dry_run(&self, _: Invocation) -> Result<Preview, AppRefusal> {
        Ok(Preview::None)
    }
    async fn undo(&self, _: UndoToken, _: Actor) -> Result<(), UndoFault> {
        Ok(())
    }
    async fn search(&self, _: &str) -> Vec<Hit> {
        vec![]
    }
    async fn preview(&self, _: &EntityId) -> Preview {
        Preview::None
    }
    async fn suggest(&self, _: SuggestAsk) -> Vec<EntityRef> {
        vec![]
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_activation_token_rides_beside_the_invocation_to_the_provider() {
    let (dir, bus, daemon, link) = bus_with_intentd().await;
    let connection = bus.connect().await;
    let who = answering_intentd(&connection, &daemon, dir.path());
    let manifest = validate(Manifest {
        vocab: IntentsVocab(1),
        app: app("org.quire.Keeps"),
        entities: vec![],
        actions: vec![],
        visibility: Visibility::Everyone,
    })
    .expect("a manifest");
    let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    serve_on_with(
        &connection,
        Keeps(manifest, seen.clone()),
        Blank(app("org.quire.Keeps")),
        Blank(app("org.quire.Keeps")),
        who,
    )
    .await
    .expect("serves");
    let keeps = app("org.quire.Keeps");
    let token = ActivationToken::parse("xdg-activation-77").expect("token");
    let _ = link
        .perform_activated(
            &keeps,
            invocation("keeps.do", "x"),
            Some(token.clone()),
            Latency::Instant,
        )
        .await;
    let _ = link
        .perform(&keeps, invocation("keeps.do", "x"), Latency::Instant)
        .await;
    assert_eq!(*seen.lock().expect("log"), [Some(token), None]);
}
