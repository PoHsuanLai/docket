//! The system clock reads the time, and a wait of no time at all is done at once.

use docket_store::SystemClock;
use docket_vocab::{Clock, Millis};
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

#[test]
fn the_clock_reads_a_time_after_the_epoch() {
    assert!(SystemClock.now().0 > 0);
}

#[test]
fn no_time_at_all_is_already_done() {
    let mut waiting = Box::pin(SystemClock.after(Millis(0)));
    let mut cx = Context::from_waker(Waker::noop());
    assert_eq!(Pin::new(&mut waiting).poll(&mut cx), Poll::Ready(()));
}
