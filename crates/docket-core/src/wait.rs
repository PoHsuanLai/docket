//! Waiting on another process without waiting forever. Every await on a peer (a model, the router,
//! an agent child, a reader) goes through here: [`within`] gives it a deadline, [`until`] a
//! deadline and a [`Cancel`], and [`Waits`] is the typed config the spans come from. Nothing here
//! names a runtime: a host hands in the future that sleeps (its own timer, or [`thread_sleep`]
//! where it has none).

use futures_util::future::{Either, select};
use futures_util::task::AtomicWaker;
use std::future::Future;
use std::pin::pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::Poll;
use std::time::Duration;

pub use crate::wait_timer::{ThreadSleep, thread_sleep};

/// How long each kind of peer may take, from config with these defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Waits {
    /// A peer that should answer at once: a router call, a reader ask, a write to a child.
    pub prompt: Duration,
    /// A model turn, from the request to the end of its reply.
    pub model: Duration,
    /// A wait that has the person in it: a confirmation sheet is up.
    pub person: Duration,
    /// An agent child starting: its handshake and session set-up.
    pub handshake: Duration,
    /// Start-up work before a service answers: rebuilding what a restart finds.
    pub startup: Duration,
}

impl Waits {
    /// The defaults: a minute for what should be quick, three minutes for a model, a quarter of
    /// an hour for a person.
    pub const DEFAULT: Waits = Waits {
        prompt: Duration::from_secs(60),
        model: Duration::from_secs(180),
        person: Duration::from_secs(15 * 60),
        handshake: Duration::from_secs(60),
        startup: Duration::from_secs(30),
    };
}

impl Default for Waits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// Why a wait ended without the peer answering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Stop {
    /// The span ran out.
    #[error("no answer in time")]
    TimedOut,
    /// The person stopped the task.
    #[error("cancelled")]
    Cancelled,
}

#[derive(Debug, Default)]
struct Flag {
    set: AtomicBool,
    waker: AtomicWaker,
}

/// A stop a task's waits listen for. Clones share the flag; one waiter at a time listens.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    flag: Arc<Flag>,
}

impl Cancel {
    /// Stops whoever waits on this, now and until it is dropped.
    pub fn cancel(&self) {
        self.flag.set.store(true, Ordering::SeqCst);
        self.flag.waker.wake();
    }

    /// Whether it was stopped.
    pub fn is_set(&self) -> bool {
        self.flag.set.load(Ordering::SeqCst)
    }

    /// Resolves once it is stopped.
    pub async fn cancelled(&self) {
        std::future::poll_fn(|cx| {
            if self.is_set() {
                return Poll::Ready(());
            }
            self.flag.waker.register(cx.waker());
            if self.is_set() {
                Poll::Ready(())
            } else {
                Poll::Pending
            }
        })
        .await
    }
}

/// The first of a sleep and a cancel; a cancel that is already there wins a tie.
pub async fn stop_after(sleep: impl Future<Output = ()>, cancel: impl Future<Output = ()>) -> Stop {
    let sleep = pin!(sleep);
    let cancel = pin!(cancel);
    match select(cancel, sleep).await {
        Either::Left(_) => Stop::Cancelled,
        Either::Right(_) => Stop::TimedOut,
    }
}

/// `work`, unless `stop` resolves first, which drops it.
pub async fn until<T>(
    work: impl Future<Output = T>,
    stop: impl Future<Output = Stop>,
) -> Result<T, Stop> {
    let work = pin!(work);
    let stop = pin!(stop);
    match select(work, stop).await {
        Either::Left((done, _)) => Ok(done),
        Either::Right((why, _)) => Err(why),
    }
}

/// `work`, unless `sleep` resolves first, which drops it.
pub async fn within<T>(
    sleep: impl Future<Output = ()>,
    work: impl Future<Output = T>,
) -> Result<T, Stop> {
    until(work, async {
        sleep.await;
        Stop::TimedOut
    })
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::task::noop_waker;
    use std::task::Context;

    fn poll_once<F: Future + Unpin>(f: &mut F) -> Poll<F::Output> {
        let waker = noop_waker();
        Pin::new(f).poll(&mut Context::from_waker(&waker))
    }
    use std::pin::Pin;

    #[test]
    fn a_wait_ends_by_the_first_of_its_work_its_sleep_and_a_cancel() {
        let mut quick = Box::pin(within(std::future::pending(), async { 7 }));
        assert_eq!(poll_once(&mut quick), Poll::Ready(Ok(7)));

        let mut slow = Box::pin(within(async {}, std::future::pending::<u8>()));
        assert_eq!(poll_once(&mut slow), Poll::Ready(Err(Stop::TimedOut)));

        let cancel = Cancel::default();
        let mut waiting = Box::pin(until(
            std::future::pending::<u8>(),
            stop_after(std::future::pending(), cancel.cancelled()),
        ));
        assert_eq!(poll_once(&mut waiting), Poll::Pending);
        cancel.cancel();
        assert_eq!(poll_once(&mut waiting), Poll::Ready(Err(Stop::Cancelled)));
    }

    #[test]
    fn the_thread_sleep_elapses() {
        let mut sleep = Box::pin(thread_sleep(Duration::from_millis(10)));
        assert_eq!(poll_once(&mut sleep), Poll::Pending);
        std::thread::sleep(Duration::from_millis(200));
        assert_eq!(poll_once(&mut sleep), Poll::Ready(()));
    }
}
