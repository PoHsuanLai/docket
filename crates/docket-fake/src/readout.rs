//! Stand-in readouts for the shadow flagger: no model, deterministic, and offline.

use crate::scripted::Forget;
use action_review::{ArgView, OptionScores, Readout, ReviewRequest, ShadowFault, Shadowed};
use action_review::{Reviewer, ShadowFlagger, ShadowSink};
use porter_core::Permille;
use prov::Effect;

/// Always gives the same P(flag) to the option named `flag`, the rest to the others.
#[derive(Debug, Clone, Copy)]
pub struct FixedReadout(pub Permille);

impl Readout for FixedReadout {
    async fn read(
        &self,
        _request: &ReviewRequest,
        options: &[String],
    ) -> Result<OptionScores, ShadowFault> {
        let rest = options.len().saturating_sub(1).max(1) as u64;
        let left = u64::from(1000u32.saturating_sub(self.0.0));
        OptionScores::from_weights(
            options
                .iter()
                .map(|o| match o.as_str() {
                    "flag" => (o.clone(), u64::from(self.0.0)),
                    _ => (o.clone(), left / rest),
                })
                .collect(),
        )
    }
}

/// A readout that cannot be read.
#[derive(Debug, Clone, Copy)]
pub struct BrokenReadout;

impl Readout for BrokenReadout {
    async fn read(
        &self,
        _request: &ReviewRequest,
        _options: &[String],
    ) -> Result<OptionScores, ShadowFault> {
        Err(ShadowFault::Unreadable)
    }
}

/// P(flag) from what the stripped request plainly shows: somebody else's words in an argument
/// raise it a lot, a destructive or outbound effect a little. A stand-in with the right shape
/// for the eval report, not a judge.
#[derive(Debug, Clone, Copy)]
pub struct FeatureReadout;

fn flag_odds(request: &ReviewRequest) -> u32 {
    let untrusted = request
        .proposed
        .args
        .iter()
        .any(|(_, _, a)| matches!(a, ArgView::Untrusted { .. }));
    let by_effect = match request.proposed.effect {
        Effect::Read | Effect::UndoableWrite => 0,
        Effect::Outbound | Effect::Destructive | Effect::Execute => 200,
    };
    80 + by_effect + if untrusted { 520 } else { 0 }
}

impl Readout for FeatureReadout {
    async fn read(
        &self,
        request: &ReviewRequest,
        options: &[String],
    ) -> Result<OptionScores, ShadowFault> {
        FixedReadout(Permille(flag_odds(request)))
            .read(request, options)
            .await
    }
}

impl<R, F: ShadowFlagger, S: ShadowSink> Forget for Shadowed<R, F, S>
where
    R: Reviewer + Forget,
{
    fn forget(&self) {
        self.live.forget();
    }
}
