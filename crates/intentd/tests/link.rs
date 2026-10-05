//! The apps over a real bus: `DbusLink` on intentd's side, the real `docket_client::serve_on` on
//! the app's, with docket-fake's mail app behind it. A provider answers nothing but intentd, an
//! app that is away is "unavailable" rather than a panic, and a slow one is cut at its latency.

mod support;

use docket_client::{ContextSource, IntentProvider, SummonTarget, serve_on};
use docket_core::*;
use docket_dbus::{INTENTS_BUS, IntentProviderProxy};
use docket_router::{AppFault, AppLink, LinkFault};
use intentd::DbusLink;
use prov::{Actor, EntityId, EntityKey, EntityKind, SpaceId};
use std::time::Duration;
use support::apps::{Blank, serve_mail};
use support::bus::PrivateBus;
use support::world::app;

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
        activation: None,
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
    let (_dir, bus, _daemon, link) = bus_with_intentd().await;
    let app_connection = bus.connect().await;
    let mail_app = serve_mail(&app_connection).await;

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

    // Context, search, preview and suggestions.
    let context = link
        .context(&mail(), ContextScope::ActiveWindow)
        .await
        .expect("a snapshot");
    assert_eq!(context.app, mail());
    assert!(
        link.search(&mail(), "invoice", Generation(1)).await.is_ok(),
        "search"
    );
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
        link.search(&gone, "x", Generation(1)).await,
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
        tokio::time::sleep(Duration::from_secs(3)).await;
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
    let (_dir, bus, _daemon, link) = bus_with_intentd().await;
    let slow_connection = bus.connect().await;
    let manifest = validate(Manifest {
        vocab: IntentsVocab(1),
        app: app("org.quire.Slow"),
        entities: vec![],
        actions: vec![],
    })
    .expect("a manifest");
    serve_on(
        &slow_connection,
        Slow(manifest),
        Blank(app("org.quire.Slow")),
        Blank(app("org.quire.Slow")),
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
    assert_eq!(
        link.search(&slow, "x", Generation(1)).await,
        Err(LinkFault::Timeout)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_answers_nobody_but_intentd() {
    let (_dir, bus, _daemon, _link) = bus_with_intentd().await;
    let app_connection = bus.connect().await;
    serve_mail(&app_connection).await;
    // A terminal, another app, anything that is not the owner of intentd's name calling the
    // app's bus name directly gets no way around the gate.
    let stranger = bus.connect().await;
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
