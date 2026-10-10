//! The shipped [`Prepare`]: porter-client's `Transport::prepare` over a private bus, as the
//! intentd unit (the policy writer's identity, which inferd serves every tier of a run to).

use crate::live::warm::{POLL, Prepare, WarmFault, plan, warm_all};
use crate::world::{Cgroup, ModelSource, World, place, restore_unit, unit_main};
use porter_client::{DbusTransport, OpenOptions, Transport};
use porter_core::capability::LlmFeature;
use porter_core::consent::Usage;
use porter_core::need::LlmNeed;
use porter_core::{DataClass, Need, Tier, Tokens};
use porter_infer::Readiness;
use std::collections::BTreeSet;
use std::time::{Duration, Instant};

/// Asks inferd over `connection`.
#[derive(Debug)]
pub struct BusPrepare {
    transport: DbusTransport,
    began: Instant,
}

impl BusPrepare {
    /// Prepares over `connection`.
    pub fn over(connection: zbus::Connection) -> Self {
        Self {
            transport: DbusTransport::over(connection),
            began: Instant::now(),
        }
    }
}

fn need() -> Need {
    Need::Llm(LlmNeed::new(
        BTreeSet::from([LlmFeature::Chat, LlmFeature::StructuredOutput]),
        Tokens(2_000),
    ))
}

impl Prepare for BusPrepare {
    async fn prepare(&self, tier: Tier) -> Option<Readiness> {
        self.transport
            .prepare(
                &need(),
                DataClass::Prompt,
                tier,
                &OpenOptions::default().with_usage(Usage::Interactive),
            )
            .await
            .ok()
    }

    async fn pause(&self) {
        tokio::time::sleep(POLL).await;
    }

    fn now(&self) -> Duration {
        self.began.elapsed()
    }
}

/// Warms every model `source` routes to over `connection` (a cassette routes to none, and this
/// does nothing), printing each as it is ready. The caller is placed as intentd.
pub async fn warm_up(
    source: &ModelSource,
    connection: zbus::Connection,
    patience: Duration,
) -> Result<(), WarmFault> {
    let steps = plan(source)?;
    let inferd = BusPrepare::over(connection);
    warm_all(&steps, &inferd, patience, |w| eprintln!("{}", w.line()))
        .await
        .map(drop)
}

/// Runs `work` with the harness process standing in as the intentd unit, then puts both back:
/// the harness in the shell's scope, and the unit's main pid with the daemon that really is
/// intentd (a service caller is known by its unit's main pid, so leaving the harness there would
/// make memoryd refuse every record of the real intentd).
pub async fn as_intentd<T>(world: &World, work: impl std::future::Future<Output = T>) -> T {
    let (root, pid) = (world.dir.path(), std::process::id());
    let daemon = unit_main(root, "intentd");
    place(root, pid, Cgroup::Unit("intentd"));
    let done = work.await;
    place(root, pid, Cgroup::Scope("sill-shell"));
    restore_unit(root, "intentd", daemon);
    done
}

/// Warms a smoke world's models. The harness process stands in for sill there; for the warm-up
/// it is placed as intentd (the identity inferd serves a run's models to) and put back after.
pub async fn warm_world(
    world: &World,
    source: &ModelSource,
    patience: Duration,
) -> Result<(), WarmFault> {
    if matches!(source, ModelSource::Scripted(_)) {
        return Ok(());
    }
    let connection = world.connect().await;
    as_intentd(world, warm_up(source, connection, patience)).await
}
