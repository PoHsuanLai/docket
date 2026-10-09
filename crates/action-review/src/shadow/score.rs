//! What a shadow flagger can say: a score, and from it a lean that cannot be an Allow.

use porter_core::Permille;
use serde::{Deserialize, Serialize};

/// The probability, in thousandths, that the Quick judge should flag the request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ShadowScore(pub Permille);

/// The score at or above which the shadow would flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Threshold(pub Permille);

/// The shipped cut. The decision-model tests found 0.5 too high for ranking scores and 0.12 too
/// low to transfer; this is where a trial starts, and the eval report shows others.
pub const DEFAULT_THRESHOLD: Threshold = Threshold(Permille(300));

/// What the shadow would have done. Deliberately not a verdict: there is no way to read an
/// Allow out of it, and no conversion into one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShadowLean {
    /// It would have let the Quick judge's pass stand.
    WouldPass,
    /// It would have flagged.
    WouldFlag,
}

impl ShadowScore {
    /// The lean at `threshold`: a flag from the threshold up.
    pub fn lean(self, threshold: Threshold) -> ShadowLean {
        if self.0.0 >= threshold.0.0 {
            ShadowLean::WouldFlag
        } else {
            ShadowLean::WouldPass
        }
    }
}

/// Why a shadow has no score. None of these touches the live verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, thiserror::Error)]
#[serde(rename_all = "snake_case")]
pub enum ShadowFault {
    /// No flagger is configured (the default).
    #[error("no shadow flagger is configured")]
    Off,
    /// The scorer gave nothing usable.
    #[error("the shadow scorer's answer was unreadable")]
    Unreadable,
    /// The scorer could not be reached.
    #[error("the shadow scorer was unavailable")]
    Unavailable,
    /// The live judge had answered before the scorer did.
    #[error("the shadow scorer was slower than the live judge")]
    Late,
}
