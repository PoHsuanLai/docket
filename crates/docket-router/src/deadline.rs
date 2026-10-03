//! The deadline a reviewer stage runs under: the router races the stage against the clock's
//! `after`, so a hung model costs its stage's time and no more. The pure `action-review` crate
//! owns no timer; this is where the time limit is enforced.

use std::future::{Future, poll_fn};
use std::pin::pin;
use std::task::Poll;

/// `work`, or `None` if `timer` completes first. The timer is polled first, so a stage with no
/// time at all never starts.
pub(crate) async fn within<T>(
    timer: impl Future<Output = ()>,
    work: impl Future<Output = T>,
) -> Option<T> {
    let mut timer = pin!(timer);
    let mut work = pin!(work);
    poll_fn(|cx| {
        if timer.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        work.as_mut().poll(cx).map(Some)
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::within;
    use std::future::{pending, ready};

    fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        use std::task::{Context, Poll, Waker};
        let mut f = std::pin::pin!(f);
        let mut cx = Context::from_waker(Waker::noop());
        match f.as_mut().poll(&mut cx) {
            Poll::Ready(v) => v,
            Poll::Pending => panic!("the future did not finish"),
        }
    }

    #[test]
    fn the_timer_wins_ties_and_hangs_but_not_a_finished_stage() {
        assert_eq!(block(within(ready(()), ready(1))), None, "no time at all");
        assert_eq!(
            block(within(ready(()), pending::<u8>())),
            None,
            "a hung stage"
        );
        assert_eq!(block(within(pending::<()>(), ready(7))), Some(7));
    }
}
