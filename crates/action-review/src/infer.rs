//! The model-backed reviewer: one model per stage over porter-infer's `Model` seam, with the
//! stage's timeout. A timeout or any failure is an error, which `tighten` turns into a
//! confirmation; the reviewer never allows by failing.

use crate::cascade::Reviewer;
use crate::request::ReviewRequest;
use crate::verdict::ReviewVerdict;
use docket_core::{ReviewError, ReviewTimeouts, Stage};
use porter_infer::Model;

/// Reviews with three models, one per stage.
#[derive(Debug)]
pub struct InferReviewer<M> {
    /// The quick judge.
    pub quick: M,
    /// The deliberate model.
    pub deliberate: M,
    /// The second opinion.
    pub second: M,
    /// The stage timeouts.
    pub timeouts: ReviewTimeouts,
}

impl<M: Model> Reviewer for InferReviewer<M> {
    async fn review(
        &self,
        stage: Stage,
        request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        let _ = (
            stage,
            request,
            &self.quick,
            &self.deliberate,
            &self.second,
            &self.timeouts,
        );
        todo!(
            "InferReviewer::review: render the prompt, ask the stage's model with ReplyShape::Choice (Quick) or Json (the rest) at Usage::Interactive, parse the reply, give up at the stage timeout"
        )
    }
}
