//! The sheet over a real bus: sill's `Confirm1` is asked only when its owner is the process the
//! configuration trusts with the `confirm` role; whatever goes wrong with the person's side (no
//! sill, a sill nobody trusts, one that never answers, one that vanishes) is a dismissal or an
//! expiry, never an allow.

mod support;

use docket_core::*;
use intentd::{IntentdConfig, SheetConfirmer};
use porter_core::Count;
use prov::{Actor, ConfirmId, ConfirmReceipt, Effect, InputProof, SpaceId, UnixSeconds};
use std::sync::Arc;
use std::time::Duration;
use support::apps::{Answer, FakeSill};
use support::bus::PrivateBus;
use support::world::app;

fn request(id: &str, expires: u32) -> ConfirmRequest {
    ConfirmRequest {
        id: ConfirmId::parse(id).expect("id"),
        space: SpaceId::parse("work").expect("space"),
        actor: Actor::Cli,
        app: app("org.quire.Mail"),
        action: LabelText::parse("Archive a thread").expect("label"),
        effect: Effect::UndoableWrite,
        count: Count(1),
        detail: ConfirmDetail::Plain,
        lines: vec![],
        why: vec![AskReason::FromTerminal],
        taint: TaintNote::Clean,
        offer: ConfirmOffer::OnceOrFromTerminal,
        always: Default::default(),
        gesture: Gesture::Press,
        anchor: Anchor::Centre,
        expires: Seconds(expires),
    }
}

fn receipt() -> ConfirmReceipt {
    ConfirmReceipt {
        id: ConfirmId::parse("c-1").expect("id"),
        input: InputProof::HardwareSeat,
        at: UnixSeconds(1),
        covers: prov::Confidentiality::Secret,
    }
}

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: receipt(),
    }
}

struct Desk {
    _dir: tempfile::TempDir,
    bus: PrivateBus,
    sheet: SheetConfirmer,
}

async fn desk() -> Desk {
    let dir = tempfile::tempdir().expect("scratch");
    let bus = PrivateBus::start(dir.path());
    let daemon = bus.connect().await;
    let config = IntentdConfig::shipped().expect("the shipped configuration");
    let sheet =
        SheetConfirmer::trusting(daemon, Arc::new(config)).waiting(Duration::from_millis(100));
    Desk {
        _dir: dir,
        bus,
        sheet,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_trusted_sill_is_asked_and_its_answer_is_the_answer() {
    let desk = desk().await;
    let connection = desk.bus.connect().await;
    // The shipped configuration gives sill the name org.quire.Shell and the confirm role.
    let sill = FakeSill::start(&connection, &["org.quire.Confirm1", "org.quire.Shell"]).await;
    sill.answer(vec![
        Answer::With(yes()),
        Answer::With(ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }),
        Answer::With(ConfirmAnswer::Ended(ConfirmEnd::Refused)),
    ]);
    assert_eq!(desk.sheet.confirm(request("c-1", 120)).await, yes());
    assert_eq!(
        desk.sheet.confirm(request("c-2", 120)).await,
        ConfirmAnswer::AllowedFromTerminal { receipt: receipt() }
    );
    assert_eq!(
        desk.sheet.confirm(request("c-3", 120)).await,
        ConfirmAnswer::Ended(ConfirmEnd::Refused)
    );
    let shown = sill.shown();
    assert_eq!(shown.len(), 3);
    assert_eq!(
        shown[0],
        request("c-1", 120),
        "the whole request arrives, typed"
    );
    // Nothing queued: the person walks away.
    assert_eq!(
        desk.sheet.confirm(request("c-4", 120)).await,
        ConfirmAnswer::Ended(ConfirmEnd::Dismissed)
    );
    desk.sheet
        .cancel(&ConfirmId::parse("c-4").expect("id"))
        .await;
    assert_eq!(sill.cancelled(), ["c-4"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_nobody_trusted_is_never_asked_and_never_allows() {
    let desk = desk().await;
    let connection = desk.bus.connect().await;
    // Owns org.quire.Confirm1 but plays no role in the configuration.
    let impostor = FakeSill::start(&connection, &["org.quire.Confirm1"]).await;
    impostor.answer(vec![Answer::With(yes())]);
    assert_eq!(
        desk.sheet.confirm(request("c-1", 120)).await,
        ConfirmAnswer::Ended(ConfirmEnd::Dismissed),
        "an answer from a process that is not sill is no answer"
    );
    assert!(impostor.shown().is_empty(), "the request never reached it");
    desk.sheet
        .cancel(&ConfirmId::parse("c-1").expect("id"))
        .await;
    assert!(impostor.cancelled().is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_sill_is_a_dismissal() {
    let desk = desk().await;
    assert_eq!(
        desk.sheet.confirm(request("c-1", 120)).await,
        ConfirmAnswer::Ended(ConfirmEnd::Dismissed)
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sheet_nobody_answers_expires_and_a_sill_that_vanishes_dismisses() {
    let desk = desk().await;
    let connection = desk.bus.connect().await;
    let sill = FakeSill::start(&connection, &["org.quire.Confirm1", "org.quire.Shell"]).await;
    sill.answer(vec![Answer::Never, Answer::Never]);
    // Its own expiry (0 s) plus the grace: expired, not allowed.
    assert_eq!(
        desk.sheet.confirm(request("c-1", 0)).await,
        ConfirmAnswer::Ended(ConfirmEnd::Expired)
    );
    // The sheet is up and sill goes away: dismissed, at once.
    let waiting = tokio::spawn({
        let sheet = desk.sheet.clone();
        async move { sheet.confirm(request("c-2", 120)).await }
    });
    tokio::time::sleep(Duration::from_millis(200)).await;
    connection.close().await.expect("close sill");
    let ended = tokio::time::timeout(Duration::from_secs(5), waiting)
        .await
        .expect("it does not hang")
        .expect("joined");
    assert_eq!(ended, ConfirmAnswer::Ended(ConfirmEnd::Dismissed));
}
