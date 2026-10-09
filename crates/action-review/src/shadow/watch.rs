//! The shadowed reviewer: the live reviewer's verdict, with a note kept beside the Quick stage.

use super::flagger::ShadowFlagger;
use super::note::{LiveCall, ShadowNote, ShadowSink};
use super::score::ShadowFault;
use crate::cascade::Reviewer;
use crate::request::ReviewRequest;
use crate::verdict::ReviewVerdict;
use docket_core::{ReviewError, ShadowMode, Stage};
use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

/// A reviewer that runs a flagger beside the live Quick stage and keeps a note of both. The
/// verdict it returns is the live reviewer's, always: the flagger runs alongside, never in
/// front, and the live result is returned the moment it is ready. A flagger that has not
/// answered by then is noted `Late` and dropped, so it cannot delay the stage into its deadline
/// either. Other stages, and `ShadowMode::Off`, go straight to the live reviewer.
#[derive(Debug)]
pub struct Shadowed<R, F, S> {
    /// The reviewer that decides.
    pub live: R,
    /// The scorer that is only recorded.
    pub flagger: F,
    /// Where the notes go.
    pub sink: S,
    /// Whether the shadow runs at all.
    pub mode: ShadowMode,
}

impl<R: Reviewer, F: ShadowFlagger, S: ShadowSink> Reviewer for Shadowed<R, F, S> {
    async fn review(
        &self,
        stage: Stage,
        request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        match (stage, self.mode) {
            (Stage::Quick, ShadowMode::Record) => {}
            _ => return self.live.review(stage, request).await,
        }
        let mut live = pin!(self.live.review(stage, request));
        let mut shadow = pin!(self.flagger.score(request));
        let mut scored = None;
        let verdict = poll_fn(|cx| {
            if scored.is_none()
                && let Poll::Ready(s) = shadow.as_mut().poll(cx)
            {
                scored = Some(s);
            }
            live.as_mut().poll(cx)
        })
        .await;
        match scored.unwrap_or(Err(ShadowFault::Late)) {
            Err(ShadowFault::Off) => {}
            shadow => self.sink.record(ShadowNote {
                live: LiveCall::of(&verdict),
                shadow,
            }),
        }
        verdict
    }
}
