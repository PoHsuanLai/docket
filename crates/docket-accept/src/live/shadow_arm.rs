//! The shadow flagger a corpus run uses. Only a scripted run has a readout (a deterministic
//! stand-in); a local or cloud run has none until porter returns option probabilities, so its
//! arm is the disabled one and the report says it scored nothing.

use crate::live::engine::Engine;
use action_review::{
    Disabled, OptionFlagger, ReviewRequest, ShadowFault, ShadowFlagger, ShadowScore,
};
use docket_fake::FeatureReadout;

/// The arm a run picks.
#[derive(Debug, Clone, Copy)]
pub enum Arm {
    /// A stand-in readout.
    Stand(OptionFlagger<FeatureReadout>),
    /// None.
    None(Disabled),
}

impl Arm {
    /// The arm for an engine.
    pub fn for_engine(engine: Engine) -> Self {
        match engine {
            Engine::Scripted => Self::Stand(OptionFlagger(FeatureReadout)),
            Engine::Local | Engine::Cloud => Self::None(Disabled),
        }
    }
}

impl ShadowFlagger for Arm {
    async fn score(&self, request: &ReviewRequest) -> Result<ShadowScore, ShadowFault> {
        match self {
            Self::Stand(flagger) => flagger.score(request).await,
            Self::None(flagger) => flagger.score(request).await,
        }
    }
}
