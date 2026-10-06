//! Running a future to its end with no runtime. The fakes answer at once, so a future the
//! runner polls is ready on its first poll; a live run's futures wait on a bus or a model, and
//! the thread parks until the future's waker says it can be polled again. The caller keeps a
//! runtime entered when the future needs timers.

use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::task::{Context, Poll, Wake, Waker};
use std::thread::Thread;

struct Unpark(Thread);

impl Wake for Unpark {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}

/// The output of `future`, polled on this thread.
pub(crate) fn block_on<F: Future>(future: F) -> F::Output {
    let mut future = pin!(future);
    let waker = Waker::from(Arc::new(Unpark(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(out) = future.as_mut().poll(&mut cx) {
            return out;
        }
        std::thread::park();
    }
}
