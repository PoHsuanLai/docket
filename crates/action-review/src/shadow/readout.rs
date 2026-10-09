//! The option-probability readout: a distribution over the options a question declares, taken
//! from the model that already answers it (the openjev pattern). porter-infer does not return
//! token log-probabilities yet; until it does, a `Readout` is a seam with fakes (see
//! `docket-fake`) and the porter ask is in FINDINGS.

use super::score::ShadowFault;
use crate::request::ReviewRequest;
use porter_core::Permille;
use std::future::Future;

/// A probability per declared option, in thousandths, summing to 1000.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionScores(Vec<(String, Permille)>);

impl OptionScores {
    /// Normalises non-negative `weights` (probabilities, odds or counts) to a distribution. The
    /// rounding remainder goes to the heaviest option. Nothing to share out is unreadable.
    pub fn from_weights(weights: Vec<(String, u64)>) -> Result<Self, ShadowFault> {
        let total: u64 = weights.iter().map(|(_, w)| *w).sum();
        if total == 0 {
            return Err(ShadowFault::Unreadable);
        }
        let mut shares: Vec<(String, u64, u64)> = weights
            .into_iter()
            .map(|(option, w)| (option, w, w.saturating_mul(1000) / total))
            .collect();
        let given: u64 = shares.iter().map(|(_, _, p)| *p).sum();
        if let Some(top) = shares.iter_mut().max_by_key(|(_, w, _)| *w) {
            top.2 += 1000 - given;
        }
        Ok(Self(
            shares
                .into_iter()
                .map(|(option, _, p)| (option, Permille(u32::try_from(p).unwrap_or(1000))))
                .collect(),
        ))
    }

    /// The probability of `option`, if it was declared.
    pub fn of(&self, option: &str) -> Option<Permille> {
        self.0.iter().find(|(o, _)| o == option).map(|(_, p)| *p)
    }
}

/// Reads the distribution over `options` for a request.
pub trait Readout: Send + Sync {
    /// The scores for `options`, or why there are none.
    fn read(
        &self,
        request: &ReviewRequest,
        options: &[String],
    ) -> impl Future<Output = Result<OptionScores, ShadowFault>> + Send;
}
