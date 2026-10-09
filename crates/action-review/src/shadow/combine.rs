//! The later combined mode: either one flags. It can only escalate.

use super::score::ShadowLean;
use crate::verdict::{ReviewReason, ReviewVerdict};
use docket_core::{ReasonCode, ReasonText, ReviewError};

/// The live Quick result with the shadow's lean folded in. A pass the shadow would flag becomes
/// an ask; everything else (an ask, a refusal, a failure, a pass the shadow agrees with) is
/// returned as it came. The lean has no allow in it, so the result is never looser than `live`.
pub fn either_flags(
    live: Result<ReviewVerdict, ReviewError>,
    lean: ShadowLean,
) -> Result<ReviewVerdict, ReviewError> {
    match (live, lean) {
        (Ok(ReviewVerdict::Allow), ShadowLean::WouldFlag) => Ok(ReviewVerdict::Ask {
            why: ReviewReason {
                code: ReasonCode::Uncertain,
                text: ReasonText("the second scorer flagged it".to_owned()),
            },
        }),
        (live, ShadowLean::WouldFlag | ShadowLean::WouldPass) => live,
    }
}
