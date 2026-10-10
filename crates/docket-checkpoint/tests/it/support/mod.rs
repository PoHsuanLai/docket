//! Fixtures: paths, listings and a future that never waits.

use docket_checkpoint::{EntryId, TreeListing};
use docket_core::WorkPath;
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// Polls a future that never waits.
pub fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
    }
}

pub fn path(text: &str) -> WorkPath {
    WorkPath::parse(text).expect("path")
}

pub fn paths(texts: &[&str]) -> Vec<WorkPath> {
    texts.iter().map(|t| path(t)).collect()
}

/// A listing from `(path, content id)` pairs, in any order.
pub fn listing(files: &[(&str, &str)]) -> TreeListing {
    TreeListing(
        files
            .iter()
            .map(|(p, id)| (path(p), EntryId::new(*id)))
            .collect(),
    )
}
