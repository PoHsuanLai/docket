//! Failure isolation for one helper's agent: work that panics is that helper's loss and nothing
//! else's (swarm.md, decision D2). The caller drops what the work held and answers `lost`.

use futures_util::FutureExt;
use std::future::Future;
use std::panic::AssertUnwindSafe;

/// How a piece of work ended.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Caught<T> {
    /// It finished.
    Done(T),
    /// It panicked; this is the answer standing in for it.
    Lost(T),
}

/// Runs `work` to its end, or to its panic.
pub(crate) async fn caught<T>(work: impl Future<Output = T>, lost: T) -> Caught<T> {
    match AssertUnwindSafe(work).catch_unwind().await {
        Ok(done) => Caught::Done(done),
        Err(_) => Caught::Lost(lost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn work_that_panics_is_lost_alone_and_work_that_finishes_is_not() {
        let fine = caught(async { 1 }, 0).now_or_never();
        assert_eq!(fine, Some(Caught::Done(1)));
        let boom = caught(async { panic!("the agent failed") }, 0).now_or_never();
        assert_eq!(boom, Some(Caught::Lost(0)));
    }
}
