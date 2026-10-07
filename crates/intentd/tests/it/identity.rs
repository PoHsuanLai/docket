//! Who a connection is, over a real bus and a fake proc root, and what the launcher's activation
//! token does on the way to the app. The test process is every connection, so the fake
//! `/proc/<pid>/cgroup` says what the nameless ones are.

use crate::support::world::*;
use docket_client::{Intents, Transport};
use docket_core::{
    ActivationToken, Args, CallRequest, IntentsReply, IntentsRequest, JournalFilter, Origin,
    TargetValue, WireRefusal,
};
use intentd::IntentdConfig;
use prov::ActionName;

fn token() -> ActivationToken {
    ActivationToken::parse("xdg-activation-4242").expect("token")
}

fn read_t2() -> CallRequest {
    CallRequest {
        action: docket_core::ActionRef {
            app: app("org.quire.Mail"),
            name: ActionName::parse("mail.thread.read").expect("action"),
        },
        target: TargetValue::Entities(vec![prov::EntityId {
            app: app("org.quire.Mail"),
            kind: prov::EntityKind::parse("mail.thread").expect("kind"),
            key: prov::EntityKey::parse("t2").expect("key"),
        }]),
        args: Args::new(),
        origin: Origin::Launcher,
    }
}

fn shipped() -> IntentdConfig {
    IntentdConfig::shipped().expect("the shipped configuration")
}

fn performed(world: &World) -> Vec<Option<ActivationToken>> {
    world
        .router
        .seams
        .link
        .performed
        .lock()
        .expect("log")
        .iter()
        .map(|i| i.activation.clone())
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_launchers_token_crosses_the_bus_to_the_app_and_the_cli_loses_it() {
    let world = World::serving_in(mail_router(), shipped(), "vte-spawn-1.scope").await;

    // sill owns org.quire.Shell: the launcher. The token goes through D-Bus options.
    let (sill, _keep_sill) = world.client(&["org.quire.Shell"]).await;
    let sill = Intents::over(sill);
    let done = sill
        .perform_activated(read_t2(), None, None, Some(token()))
        .await
        .expect("a reply");
    assert!(done.is_ok(), "{done:?}");
    assert_eq!(performed(&world), [Some(token())]);

    // A terminal's child owns no name; its cgroup makes it the cli role. Its token is dropped,
    // and the call still runs.
    let (terminal, _keep) = world.client(&[]).await;
    let terminal = Intents::over(terminal);
    let done = terminal
        .perform_activated(read_t2(), None, None, Some(token()))
        .await
        .expect("a reply");
    assert!(done.is_ok(), "{done:?}");
    assert_eq!(performed(&world), [Some(token()), None]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_unnamed_connection_is_the_cli_only_in_a_terminal_scope() {
    /// What a nameless connection in `leaf` is.
    #[derive(Debug, PartialEq, Eq)]
    enum Is {
        Cli,
        PlainApp,
        Nobody,
    }
    for (leaf, expected) in [
        ("vte-spawn-9.scope", Is::Cli),
        ("tmux-spawn-9.scope", Is::Cli),
        ("session-2.scope", Is::Cli),
        ("app-org.example.Thing-5.scope", Is::PlainApp),
        ("intentd.service", Is::Nobody),
        ("run-r1.scope", Is::Nobody),
    ] {
        let world = World::serving_in(mail_router(), shipped(), leaf).await;
        let (nameless, _keep) = world.client(&[]).await;
        let refused = IntentsReply::Refused(WireRefusal::NotAllowed);
        let manifests = nameless
            .call(IntentsRequest::Manifests)
            .await
            .expect("a reply");
        // A journal is the cli's to read (its own rows) and a plain app's to be refused.
        let journal = nameless
            .call(IntentsRequest::ControlJournal(JournalFilter {
                run: None,
                session: None,
                limit: porter_core::Count(5),
            }))
            .await
            .expect("a reply");
        let found = match (manifests == refused, journal == refused) {
            (true, _) => Is::Nobody,
            (false, true) => Is::PlainApp,
            (false, false) => Is::Cli,
        };
        assert_eq!(found, expected, "{leaf}: {manifests:?} {journal:?}");
    }
}
