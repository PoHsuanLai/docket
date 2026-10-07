//! Warming a live engine before a run starts. A local model's first request waits for its load
//! (75 to 200 s for an 8B on vLLM), longer than any product timeout; the run asks inferd to
//! prepare every routed model first and waits for each to answer, so a flow measures what the
//! model decides and not how fast the disk is. The plan and the wait are pure: the bus is a
//! [`Prepare`] seam, a test hands in a scripted one.

use crate::world::ModelSource;
use porter_core::Tier;
use porter_infer::Readiness;
use std::future::Future;
use std::time::Duration;

/// How long to wait between two asks of one model.
pub const POLL: Duration = Duration::from_secs(2);

/// One model to bring up: the tier that routes to it and the catalogue id the config names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WarmStep {
    /// The tier a request for the model carries.
    pub tier: Tier,
    /// The model the tier routes to.
    pub model: String,
}

/// Why a model did not come up.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum WarmFault {
    /// The config is not TOML.
    #[error("the inferd config could not be read to plan the warm-up: {0}")]
    Config(String),
    /// inferd cannot serve the model at all, or refused the ask.
    #[error("{} ({:?}) is unavailable: inferd cannot serve it", .0.model, .0.tier)]
    Unavailable(WarmStep),
    /// The model was still not answering when patience ran out.
    #[error("{} ({:?}) was still {last} after {waited:?}", .step.model, .step.tier)]
    NeverCameUp {
        /// The model.
        step: WarmStep,
        /// Its last readiness, as a slug.
        last: &'static str,
        /// How long the run waited for it.
        waited: Duration,
    },
}

/// One model that came up, and how long that took.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warmed {
    /// The model.
    pub step: WarmStep,
    /// From the first ask to ready.
    pub took: Duration,
}

impl Warmed {
    /// The line a run prints for it.
    pub fn line(&self) -> String {
        format!(
            "docket-live: warm {:?} {} ready in {:.1}s",
            self.step.tier,
            self.step.model,
            self.took.as_secs_f64()
        )
    }
}

/// What a warm-up asks of inferd.
pub trait Prepare {
    /// `Inference1.Prepare` for `tier`: how ready the model is; `None` when inferd refused or is
    /// not there.
    fn prepare(&self, tier: Tier) -> impl Future<Output = Option<Readiness>>;
    /// Waits one [`POLL`].
    fn pause(&self) -> impl Future<Output = ()>;
    /// Time on a monotonic clock, for the report.
    fn now(&self) -> Duration;
}

/// The models to bring up for `source`, in the order to ask: a cassette needs none. A model that
/// two tiers share is asked once, under the first tier named. Models that cannot be resident
/// together evict one another, so the tier the first request uses (fast) is asked last.
pub fn plan(source: &ModelSource) -> Result<Vec<WarmStep>, WarmFault> {
    let ModelSource::Live(body) = source else {
        return Ok(Vec::new());
    };
    let table: toml::Table = toml::from_str(body).map_err(|e| WarmFault::Config(e.to_string()))?;
    let text = table
        .get("ai")
        .and_then(|v| v.get("model"))
        .and_then(|v| v.get("text"));
    let routed = |key: &str| text.and_then(|t| t.get(key)).and_then(|m| m.as_str());
    let mut steps: Vec<WarmStep> = Vec::new();
    for (key, tier) in [
        ("fast", Tier::Fast),
        ("balanced", Tier::Balanced),
        ("best", Tier::Best),
    ] {
        if let Some(model) = routed(key)
            && !steps.iter().any(|s| s.model == model)
        {
            steps.push(WarmStep {
                tier,
                model: model.to_owned(),
            });
        }
    }
    steps.reverse();
    Ok(steps)
}

/// How many asks patience allows one model: the first, then one per [`POLL`].
fn asks(patience: Duration) -> u64 {
    1 + patience.as_secs() / POLL.as_secs()
}

/// Brings one model up: asks until it is ready, waiting between asks.
async fn warm_one(
    step: &WarmStep,
    inferd: &impl Prepare,
    patience: Duration,
) -> Result<Warmed, WarmFault> {
    let started = inferd.now();
    let mut last = Readiness::Loadable;
    for ask in 0..asks(patience) {
        if ask > 0 {
            inferd.pause().await;
        }
        match inferd.prepare(step.tier).await {
            Some(Readiness::Ready) => {
                return Ok(Warmed {
                    step: step.clone(),
                    took: inferd.now().saturating_sub(started),
                });
            }
            None | Some(Readiness::Unavailable) => {
                return Err(WarmFault::Unavailable(step.clone()));
            }
            Some(now) => last = now,
        }
    }
    Err(WarmFault::NeverCameUp {
        step: step.clone(),
        last: last.slug(),
        waited: inferd.now().saturating_sub(started),
    })
}

/// Brings every step up in order; stops at the first model that does not come up. `said` hears
/// each one as it is ready.
pub async fn warm_all(
    steps: &[WarmStep],
    inferd: &impl Prepare,
    patience: Duration,
    mut said: impl FnMut(&Warmed),
) -> Result<Vec<Warmed>, WarmFault> {
    let mut done = Vec::new();
    for step in steps {
        let warmed = warm_one(step, inferd, patience).await?;
        said(&warmed);
        done.push(warmed);
    }
    Ok(done)
}
