//! "Until logout": a fake logind on the private bus removes the person's session, and the
//! terminal's standing grant goes with it. Another session's removal leaves it.

mod support;

use docket_core::*;
use docket_fake::ScriptedConfirmer;
use intentd::watch_logind;
use prov::{ActionName, EntityId, EntityKey, EntityKind};
use std::sync::Arc;
use std::time::Duration;
use support::bus::PrivateBus;
use support::world::{app, caller, mail_router};

fn from_terminal() -> ConfirmAnswer {
    ConfirmAnswer::AllowedFromTerminal {
        receipt: prov::ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: prov::InputProof::HardwareSeat,
            at: prov::UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

fn archive() -> CallRequest {
    CallRequest {
        action: ActionRef {
            app: app("org.quire.Mail"),
            name: ActionName::parse("mail.thread.archive").expect("action"),
        },
        target: TargetValue::Entities(vec![EntityId {
            app: app("org.quire.Mail"),
            kind: EntityKind::parse("mail.thread").expect("kind"),
            key: EntityKey::parse("t2").expect("key"),
        }]),
        args: Args::new(),
        origin: Origin::Cli,
    }
}

/// The signal logind sends when a login session ends.
async fn removed(logind: &docket_dbus::BusConnection, id: &str, number: u32) {
    let path =
        zbus::zvariant::ObjectPath::try_from(format!("/org/freedesktop/login1/session/_{number}"))
            .expect("path");
    logind
        .emit_signal(
            None::<&str>,
            "/org/freedesktop/login1",
            "org.freedesktop.login1.Manager",
            "SessionRemoved",
            &(id, path),
        )
        .await
        .expect("emit");
}

async fn until(mut done: impl FnMut() -> bool) -> bool {
    for _ in 0..100 {
        if done() {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    done()
}

async fn terminal_with_a_grant() -> Arc<docket_router::Router<docket_fake::FakeSeams>> {
    let mut router = mail_router();
    router.seams.confirmer = ScriptedConfirmer::answering(vec![from_terminal()]);
    let router = Arc::new(router);
    let cli = caller("org.quire.Do", &[CallerRole::Cli]);
    let done = router
        .handle(
            &cli,
            IntentsRequest::Perform {
                activation: None,
                call: archive(),
                session: None,
                parent_window: None,
            },
        )
        .await;
    assert!(matches!(done, IntentsReply::Performed(_)), "{done:?}");
    assert_eq!(router.terminal_grants().len(), 1);
    router
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_persons_session_ending_takes_the_terminals_grants_with_it() {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let logind = bus.connect().await;
    logind
        .request_name("org.freedesktop.login1")
        .await
        .expect("the fake logind");
    let system = bus.connect().await;
    let router = terminal_with_a_grant().await;
    let watching = watch_logind(&system).await.expect("subscribed");
    tokio::spawn(watching.end_terminals(router.clone(), Some("c1".into())));

    // Somebody else's session (an ssh login) ends: the person is still here.
    removed(&logind, "c7", 7).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        router.terminal_grants().len(),
        1,
        "another session's logout"
    );

    // The person's own ends: the grant goes, and the next call from the terminal asks again.
    removed(&logind, "c1", 1).await;
    assert!(
        until(|| router.terminal_grants().is_empty()).await,
        "grants ended"
    );
    let cli = caller("org.quire.Do", &[CallerRole::Cli]);
    let again = router
        .handle(
            &cli,
            IntentsRequest::Perform {
                activation: None,
                call: archive(),
                session: None,
                parent_window: None,
            },
        )
        .await;
    assert_eq!(
        again,
        IntentsReply::Performed(Box::new(Err(CallRefusal::Unconfirmed(
            ConfirmEnd::Dismissed
        )))),
        "it asked, nobody answered"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_no_session_id_known_any_logout_counts() {
    // Ending a grant early only means the next call from the terminal asks again.
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let logind = bus.connect().await;
    logind
        .request_name("org.freedesktop.login1")
        .await
        .expect("the fake logind");
    let system = bus.connect().await;
    let router = terminal_with_a_grant().await;
    let watching = watch_logind(&system).await.expect("subscribed");
    tokio::spawn(watching.end_terminals(router.clone(), None));
    removed(&logind, "c9", 9).await;
    assert!(
        until(|| router.terminal_grants().is_empty()).await,
        "grants ended"
    );
}
