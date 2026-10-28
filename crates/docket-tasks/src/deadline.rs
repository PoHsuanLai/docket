//! Waiting on a peer with a deadline: the two shapes every await on another process takes in the
//! loop. [`bounded`] also ends when the person cancels the task (a model turn, a read); [`timed`]
//! only runs out (a call the router may already be carrying out, which a cancel must not
//! abandon half way). Both are free functions over the shared view and the clock, so a caller can
//! hold other fields of the companion while it waits.

use crate::seams::Surface;
use crate::shared::Shared;
use docket_store::{Stop, stop_after, until, within};
use docket_vocab::{Clock, Millis};
use prov::TaskId;
use std::future::Future;
use std::time::Duration;

/// `work`, ended early by a cancel of `task` or by `span` running out.
pub(crate) async fn bounded<K: Clock, S: Surface, T>(
    shared: &Shared<S>,
    clock: &K,
    task: &TaskId,
    span: Duration,
    work: impl Future<Output = T>,
) -> Result<T, Stop> {
    let cancel = shared.cancel_of(task);
    let stop = stop_after(clock.after(millis(span)), cancel.cancelled());
    // Boxed so the waited future lives on the heap: inline it would sit one more layer deep on the
    // polling thread's stack, and the policy point's evaluator refuses to run below its stack floor.
    until(Box::pin(work), stop).await
}

/// `work`, ended early by `span` running out.
pub(crate) async fn timed<K: Clock, T>(
    clock: &K,
    span: Duration,
    work: impl Future<Output = T>,
) -> Result<T, Stop> {
    // Boxed for the same reason as in `bounded`.
    within(clock.after(millis(span)), Box::pin(work)).await
}

/// `span` in milliseconds, as many as a `Millis` holds.
fn millis(span: Duration) -> Millis {
    Millis(u32::try_from(span.as_millis()).unwrap_or(u32::MAX))
}
