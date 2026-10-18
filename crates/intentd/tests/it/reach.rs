//! A provider's live reach (`org.quire.IntentReach1`) on a private bus: the installed manifest of
//! the stand-in "mail" keeps `mail.thread.open` hidden; the provider turns it on and off, and the
//! menu a planner is given (every action that is not hidden) follows, with a `ManifestChanged`
//! each time. A provider can only switch what its manifest declares, and one that serves nothing
//! leaves the file's value.

use crate::support::bus::PrivateBus;
use crate::support::world::app;
use docket_core::{AgentConfig, AgentReach};
use docket_dbus::{BusConnection, ReachNow, ReachServer};
use docket_fake::{FakeSeams, MAIL_MANIFEST, fake_router};
use docket_router::Router;
use intentd::{Changed, LiveReach, changes, follow_reach, marks_of, parse_reach, rescan};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const OPEN: &str = "mail.thread.open";
const MAIL: &str = "org.quire.Mail";

struct Rig {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    router: Arc<Router<FakeSeams>>,
    follower: tokio::task::JoinHandle<()>,
}

impl Drop for Rig {
    fn drop(&mut self) {
        self.follower.abort();
    }
}

/// intentd following reach on a private bus, with the mail manifest installed as a file.
async fn rig() -> Rig {
    let dir = tempfile::tempdir().expect("scratch");
    let data: PathBuf = dir.path().join("data");
    let intents = data.join("quire").join("intents");
    std::fs::create_dir_all(&intents).expect("intents dir");
    std::fs::write(intents.join("org.quire.Mail.toml"), MAIL_MANIFEST).expect("manifest");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    let router = Arc::new(fake_router(AgentConfig::default()).expect("router"));
    // The fake router starts with its own fixtures; one plain load makes the installed set the
    // folder's, so what changes after it is the providers' doing alone.
    rescan(&router, std::slice::from_ref(&data));
    let follower = tokio::spawn(follow_reach(
        router.clone(),
        daemon,
        vec![data],
        LiveReach::none(),
    ));
    Rig {
        _dir: dir,
        bus,
        router,
        follower,
    }
}

/// The names a planner is offered for mail: everything not hidden.
fn menu(router: &Router<FakeSeams>) -> Vec<String> {
    let st = router.state.lock().expect("lock");
    let manifest = st.registry.get(&app(MAIL)).expect("mail is installed");
    manifest
        .manifest()
        .actions
        .iter()
        .filter(|a| a.reach != AgentReach::Hidden)
        .map(|a| a.name.as_str().to_owned())
        .collect()
}

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..100 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never happened: {what}");
}

/// A provider on its own connection: owns mail's name and serves `reach`.
async fn provider(bus: &PrivateBus, reach: &ReachServer) -> BusConnection {
    let connection = bus.connect().await;
    reach.serve(&connection).await.expect("served");
    connection.request_name(MAIL).await.expect("mail's name");
    connection
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_turns_a_hidden_action_on_and_off_and_the_menu_follows() {
    let rig = rig().await;
    until("the file is loaded", || {
        !menu(&rig.router).contains(&OPEN.to_owned()) && !menu(&rig.router).is_empty()
    })
    .await;
    let hidden = marks_of(&rig.router);

    let reach = ReachServer::new();
    reach.replace([(OPEN.to_owned(), ReachNow::Offered)]);
    let connection = provider(&rig.bus, &reach).await;
    until("the name appearing is noticed", || {
        menu(&rig.router).contains(&OPEN.to_owned())
    })
    .await;
    assert_eq!(
        changes(&hidden, &marks_of(&rig.router)),
        vec![Changed::Manifest(app(MAIL))],
        "the installed manifests changed"
    );

    reach.replace([(OPEN.to_owned(), ReachNow::Hidden)]);
    reach.announce(&connection).await.expect("announced");
    until("ReachChanged is followed", || {
        !menu(&rig.router).contains(&OPEN.to_owned())
    })
    .await;
    assert_eq!(changes(&hidden, &marks_of(&rig.router)), vec![]);

    // Gone from the bus, the file's value stands, whatever it last said.
    reach.replace([(OPEN.to_owned(), ReachNow::Offered)]);
    reach.announce(&connection).await.expect("announced");
    until("offered again", || {
        menu(&rig.router).contains(&OPEN.to_owned())
    })
    .await;
    connection.close().await.expect("closed");
    until("the provider vanishing is noticed", || {
        !menu(&rig.router).contains(&OPEN.to_owned())
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_action_the_manifest_does_not_declare_changes_nothing() {
    let rig = rig().await;
    until("the file is loaded", || !menu(&rig.router).is_empty()).await;
    let before = marks_of(&rig.router);
    let menu_before = menu(&rig.router);

    let reach = ReachServer::new();
    reach.replace([
        ("mail.nothing.here".to_owned(), ReachNow::Offered),
        ("mail.thread.read".to_owned(), ReachNow::Hidden),
    ]);
    let _connection = provider(&rig.bus, &reach).await;
    // What it declares and says is hidden does go; what it only claims does not appear.
    until("the declared action is switched", || {
        !menu(&rig.router).contains(&"mail.thread.read".to_owned())
    })
    .await;
    let mut expected = menu_before;
    expected.retain(|name| name != "mail.thread.read");
    assert_eq!(menu(&rig.router), expected);
    assert_eq!(
        changes(&before, &marks_of(&rig.router)),
        vec![Changed::Manifest(app(MAIL))]
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_provider_without_the_interface_keeps_the_files_reach() {
    let rig = rig().await;
    until("the file is loaded", || !menu(&rig.router).is_empty()).await;
    let before = marks_of(&rig.router);

    let connection = rig.bus.connect().await;
    connection.request_name(MAIL).await.expect("mail's name");
    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(!menu(&rig.router).contains(&OPEN.to_owned()));
    assert_eq!(changes(&before, &marks_of(&rig.router)), vec![]);
}

#[test]
fn only_offered_and_hidden_are_understood() {
    let said = parse_reach(r#"{"a.b":"offered","c.d":"hidden","e.f":"ask_always","g.h":3}"#);
    assert_eq!(
        said.into_iter().collect::<Vec<_>>(),
        vec![
            ("a.b".to_owned(), ReachNow::Offered),
            ("c.d".to_owned(), ReachNow::Hidden)
        ]
    );
    assert!(parse_reach("not json").is_empty());
}
