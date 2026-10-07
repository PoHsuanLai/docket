//! A watched gate check over a private bus: intentd says `Progress(Confirming(id))` before it
//! draws a sheet, waits for the run's daemon to `Proceed` (its input is suspended), draws the
//! sheet, and answers. `Close` takes the check back, and the sheet with it. An unwatched check
//! is as it was: no wait.

use crate::support::apps::{Answer, FakeSill};
use crate::support::bus::PrivateBus;
use docket_client::{DbusTransport, GateEvent, Intents};
use docket_core::*;
use intentd::{Cadence, IntentdConfig, Setup, start};
use porter_core::AppName;
use prov::{AgentRef, Effect, Label, RunId, SpaceId, UnixSeconds};
use std::collections::BTreeMap;
use std::time::Duration;

const CUAD: &str = "org.quire.Cuad";
const SILL: &str = "org.quire.Shell";

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

fn space() -> SpaceId {
    SpaceId::parse("work").expect("space")
}

fn step(run: &RunId) -> CuaAsk {
    CuaAsk {
        run: run.clone(),
        step: 1,
        app: app("org.quire.Mail"),
        trust: WindowTrust::Quire,
        mode: RunMode::InPlace,
        space: space(),
        action: cua_action::CuaAction::<cua_action::WindowSpace>::Observe,
        node: None,
        effect: Effect::Read,
        basis: EffectBasis::DefaultTable,
        screen: Label::trusted_user(),
    }
}

fn yes() -> ConfirmAnswer {
    ConfirmAnswer::Allowed {
        scope: GrantScope::Once,
        receipt: prov::ConfirmReceipt {
            id: prov::ConfirmId::parse("c-1").expect("id"),
            input: prov::InputProof::HardwareSeat,
            at: UnixSeconds(1),
            covers: prov::Confidentiality::Secret,
        },
    }
}

struct Desk {
    _dir: tempfile::TempDir,
    _bus: PrivateBus,
    sill: FakeSill,
    cuad: Intents<DbusTransport>,
    run: RunId,
    _connections: Vec<docket_dbus::BusConnection>,
    _intentd: intentd::Running,
}

impl Desk {
    async fn start() -> Desk {
        let dir = tempfile::tempdir().expect("scratch");
        let bus = PrivateBus::start(dir.path());
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
        setup.config = IntentdConfig {
            roles: BTreeMap::from([
                (CallerRole::Cua, vec![app(CUAD)]),
                (CallerRole::Confirm, vec![app(SILL)]),
            ]),
            reviewers: None,
            agent: AgentConfig::default(),
        };
        setup.signals = Cadence {
            every: Duration::from_millis(30),
            rescan_every: 2,
        };
        let daemon = bus.connect().await;
        let intentd = start(&daemon, None, setup).await.expect("intentd");
        let sill_connection = bus.connect().await;
        let sill = FakeSill::start(&sill_connection, &["org.quire.Confirm1", SILL]).await;
        let cuad_connection = bus.connect().await;
        cuad_connection.request_name(CUAD).await.expect("name");
        let cuad = Intents::over(DbusTransport::new(cuad_connection.clone()));
        let run = RunId::parse("r-1").expect("run");
        cuad.session_open(SessionOpen {
            space: space(),
            agent: AgentRef::Cua { run: run.clone() },
            parent: None,
        })
        .await
        .expect("cuad opens the session of its own run");
        Desk {
            _dir: dir,
            _bus: bus,
            sill,
            cuad,
            run,
            _connections: vec![daemon, sill_connection, cuad_connection],
            _intentd: intentd,
        }
    }
}

async fn settle() {
    tokio::time::sleep(Duration::from_millis(300)).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sheet_waits_for_proceed_and_the_verdict_follows_the_answer() {
    let desk = Desk::start().await;
    desk.sill.answer(vec![Answer::With(yes())]);
    let mut watch = desk
        .cuad
        .gate_check_watched(step(&desk.run))
        .await
        .expect("a watched check");
    let GateEvent::Confirming(id) = watch.next().await.expect("progress") else {
        panic!("the person is about to be asked")
    };
    settle().await;
    assert!(
        desk.sill.shown().is_empty(),
        "no sheet before the daemon's input is suspended"
    );
    watch.proceed().await.expect("proceed");
    let verdict = watch.next().await.expect("the verdict");
    assert_eq!(verdict, GateEvent::Verdict(GateAnswer::Run));
    let shown = desk.sill.shown();
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].id, id, "the sheet is the one it announced");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn closing_before_proceed_draws_no_sheet_and_closing_a_sheet_withdraws_it() {
    let desk = Desk::start().await;
    let mut early = desk
        .cuad
        .gate_check_watched(step(&desk.run))
        .await
        .expect("a watched check");
    assert!(matches!(
        early.next().await.expect("progress"),
        GateEvent::Confirming(_)
    ));
    early.close().await.expect("close");
    settle().await;
    assert!(
        desk.sill.shown().is_empty(),
        "no sheet for a withdrawn check"
    );

    desk.sill.answer(vec![Answer::Never]);
    let mut late = desk
        .cuad
        .gate_check_watched(step(&desk.run))
        .await
        .expect("a watched check");
    let GateEvent::Confirming(id) = late.next().await.expect("progress") else {
        panic!("confirming")
    };
    late.proceed().await.expect("proceed");
    for _ in 0..40 {
        if !desk.sill.shown().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(desk.sill.shown().len(), 1, "the sheet is up");
    late.close().await.expect("close");
    for _ in 0..40 {
        if !desk.sill.cancelled().is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(
        desk.sill.cancelled(),
        [id.to_string()],
        "the sheet came down"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unwatched_check_does_not_wait_for_anybody() {
    let desk = Desk::start().await;
    desk.sill.answer(vec![Answer::With(yes())]);
    let verdict = desk
        .cuad
        .gate_check(step(&desk.run))
        .await
        .expect("the verdict");
    assert_eq!(verdict, GateAnswer::Run);
}
