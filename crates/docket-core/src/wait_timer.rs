//! A sleep for a host that has no timer of its own: one short-lived thread per sleep, woken early
//! (and gone) when the sleep is dropped, so a wait that ends first leaves nothing behind.

use futures_util::task::AtomicWaker;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::task::{Context, Poll};
use std::time::Duration;

#[derive(Debug, Default)]
struct Gate {
    dropped: bool,
    elapsed: bool,
}

#[derive(Debug, Default)]
struct Slot {
    gate: Mutex<Gate>,
    wake: Condvar,
    waker: AtomicWaker,
}

impl Slot {
    fn elapsed(&self) -> bool {
        self.gate
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .elapsed
    }
}

/// Sleeps `span` on a thread of its own, started at the first poll.
pub fn thread_sleep(span: Duration) -> ThreadSleep {
    ThreadSleep { span, slot: None }
}

/// The future [`thread_sleep`] makes.
#[derive(Debug)]
pub struct ThreadSleep {
    span: Duration,
    slot: Option<Arc<Slot>>,
}

fn start(slot: Arc<Slot>, span: Duration) -> bool {
    std::thread::Builder::new()
        .name("wait-timer".to_owned())
        .spawn(move || {
            let gate = slot.gate.lock().unwrap_or_else(PoisonError::into_inner);
            let (mut gate, _) = slot
                .wake
                .wait_timeout_while(gate, span, |g| !g.dropped)
                .unwrap_or_else(PoisonError::into_inner);
            if !gate.dropped {
                gate.elapsed = true;
                drop(gate);
                slot.waker.wake();
            }
        })
        .is_ok()
}

impl Future for ThreadSleep {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let span = self.span;
        let slot = match &self.slot {
            Some(slot) => slot.clone(),
            None => {
                let slot = Arc::new(Slot::default());
                slot.waker.register(cx.waker());
                // No thread to wait on: the span counts as run out, so a wait never hangs on it.
                if !start(slot.clone(), span) {
                    return Poll::Ready(());
                }
                self.slot = Some(slot.clone());
                slot
            }
        };
        if slot.elapsed() {
            return Poll::Ready(());
        }
        slot.waker.register(cx.waker());
        if slot.elapsed() {
            Poll::Ready(())
        } else {
            Poll::Pending
        }
    }
}

impl Drop for ThreadSleep {
    fn drop(&mut self) {
        if let Some(slot) = &self.slot {
            slot.gate
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .dropped = true;
            slot.wake.notify_all();
        }
    }
}
