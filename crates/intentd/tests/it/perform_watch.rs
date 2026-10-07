//! A watched `Run.Perform` over a private bus: the caller says it watches (the `watch` option) and
//! hears how far its call is as it happens, a sheet announced as `Confirming(id)` as it is drawn;
//! nothing waits for the caller. An unwatched perform is as it was.

use crate::support::world::*;
use docket_client::{DbusTransport, Intents, PerformEvent};
use docket_core::*;
use intentd::IntentdConfig;
use prov::{ActionName, EntityId, EntityKey, EntityKind};
use std::collections::BTreeMap;

const TERMINAL: &str = "org.quire.Do";

fn config() -> IntentdConfig {
    IntentdConfig {
        roles: BTreeMap::from([(CallerRole::Cli, vec![app(TERMINAL)])]),
        reviewers: None,
        agent: AgentConfig::default(),
    }
}

fn delete(key: &str) -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app("org.quire.Mail"),
            name: ActionName::parse("mail.thread.delete").expect("action"),
        },
        target: TargetValue::Entities(vec![EntityId {
            app: app("org.quire.Mail"),
            kind: EntityKind::parse("mail.thread").expect("kind"),
            key: EntityKey::parse(key).expect("key"),
        }]),
        args: Args::new(),
        origin: Origin::Cli,
    }
}

async fn desk() -> (World, Intents<DbusTransport>, docket_dbus::BusConnection) {
    let world = World::serving(mail_router(), config()).await;
    let (transport, connection) = world.client(&[TERMINAL]).await;
    (world, Intents::over(transport), connection)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_watching_caller_hears_the_sheet_announced_and_then_the_end_of_its_call() {
    let (world, intents, _connection) = desk().await;
    let mut watch = intents
        .perform_watched(delete("t1"), None, None)
        .await
        .expect("a watched perform");
    let mut heard = Vec::new();
    let end = loop {
        match watch.next().await.expect("an event") {
            PerformEvent::Progress(progress) => heard.push(progress),
            PerformEvent::Done(end) => break *end,
        }
    };
    let shown = world.router.seams.confirmer.requests();
    assert_eq!(shown.len(), 1, "the person was asked once");
    assert!(
        heard.contains(&CallProgress::Confirming(shown[0].id.clone())),
        "told of the sheet it was shown: {heard:?}"
    );
    assert!(
        matches!(end, Err(CallRefusal::Unconfirmed(_))),
        "the scripted person walked away: {end:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unwatched_perform_answers_as_before() {
    let (_world, intents, _connection) = desk().await;
    let end = intents
        .perform(delete("t1"), None, None)
        .await
        .expect("an answer");
    assert!(matches!(end, Err(CallRefusal::Unconfirmed(_))), "{end:?}");
}
