//! Running a future to its end with no runtime. The fakes answer at once, so a future the
//! runner polls is ready on its first poll; a seam that really waited would be a bug here, and
//! yielding keeps it from spinning a core while it does.

use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// The output of `future`, polled on this thread.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::yield_now();
    }
}
