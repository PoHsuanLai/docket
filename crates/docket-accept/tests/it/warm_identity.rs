//! A live run warms its models with the harness standing in as the intentd unit. The stand-in
//! must give the unit back: a service caller is known by its unit's main pid, and while the
//! harness held it every call of the real intentd was unidentified, so memoryd refused its
//! audit records and no app call could be recorded (`NotRecorded`).

use crate::support::*;
use companion_wire::AnswerPhase;
use docket_accept::confirm::Verdict;
use docket_accept::drive::{Launcher, recorded};
use docket_accept::live::warm_bus::as_intentd;
use docket_accept::world::{Consent, World, unit_main};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn after_the_warm_up_the_real_intentd_is_still_the_intentd_unit() {
    let world = World::start(&binaries(), Consent::Standing, FLOW_A).await;
    let before = unit_main(world.dir.path(), "intentd");
    let during = as_intentd(&world, async { unit_main(world.dir.path(), "intentd") }).await;
    assert_ne!(during, before, "the harness is the unit while it warms");
    assert_eq!(unit_main(world.dir.path(), "intentd"), before);

    // The proof is the run itself: calls are recorded and the audit reaches memoryd.
    world.sheet.will(Verdict::Allow);
    let launcher = Launcher::of(&world).await;
    let opened = launcher.open().await;
    let mut answer = launcher
        .say(&opened, "forward the Lisbon receipts to accounting")
        .await;
    let history = answer
        .history_until(|v| {
            matches!(
                v.phase,
                AnswerPhase::Done | AnswerPhase::Failed | AnswerPhase::Cancelled
            )
        })
        .await;
    assert_eq!(
        history.last().map(|v| v.phase.clone()),
        Some(AnswerPhase::Done)
    );
    let seen = recorded(&world, &["docket.call"]).await;
    assert!(!seen.is_empty(), "memoryd took intentd's audit records");
    assert!(
        !world.logs().contains("refused the audit records"),
        "{}",
        world.logs()
    );
}
