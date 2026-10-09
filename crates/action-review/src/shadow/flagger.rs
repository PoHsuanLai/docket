//! The flagger seam and its two shipped arms: a readout of the Quick judge's own options, and
//! the disabled default a local encoder replaces later.

use super::readout::Readout;
use super::score::{ShadowFault, ShadowScore};
use crate::infer::{FLAG, quick_options};
use crate::request::ReviewRequest;
use std::future::Future;

/// Scores a stripped review request. It sees what the Quick judge sees and nothing more.
///
/// A second arm (a local encoder such as Laya) implements this too. Running one needs a
/// runtime choice (ONNX or candle), about 1 GB of VRAM beside the 4B, the 322M multilingual
/// checkpoint, and fine-tune data (synthetic `ReviewRequest`s labelled by the large model);
/// none of that is here.
pub trait ShadowFlagger: Send + Sync {
    /// The probability that the request should be flagged.
    fn score(
        &self,
        request: &ReviewRequest,
    ) -> impl Future<Output = Result<ShadowScore, ShadowFault>> + Send;
}

/// No flagger. The default; it is never asked to record anything.
#[derive(Debug, Clone, Copy, Default)]
pub struct Disabled;

impl ShadowFlagger for Disabled {
    async fn score(&self, _request: &ReviewRequest) -> Result<ShadowScore, ShadowFault> {
        Err(ShadowFault::Off)
    }
}

/// The openjev pattern: P(`flag`) from a readout over the Quick judge's own `pass`/`flag`.
#[derive(Debug, Clone, Copy)]
pub struct OptionFlagger<R>(pub R);

impl<R: Readout> ShadowFlagger for OptionFlagger<R> {
    async fn score(&self, request: &ReviewRequest) -> Result<ShadowScore, ShadowFault> {
        let scores = self.0.read(request, &quick_options()).await?;
        scores
            .of(FLAG)
            .map(ShadowScore)
            .ok_or(ShadowFault::Unreadable)
    }
}
