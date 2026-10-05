//! A requester that watches its request: what the router tells it while the call is in flight
//! (`Request.Progress`) and what it may say back (`Request.Proceed`, `Request.Close`). The
//! router is pure, so the three are callbacks the transport fills in; a request nobody watches
//! has none of them (`Watch::none`): it proceeds at once and nothing closes it.
//!
//! `Flag` and `Queue` are the two small async primitives a transport needs to build a `Watch`
//! without a runtime of its own: a flag that is raised once, and a queue read one at a time.

use docket_core::CallProgress;
use std::collections::VecDeque;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard};
use std::task::{Poll, Waker};

type Waiting = Arc<dyn Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;
type Telling = Arc<dyn Fn(CallProgress) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync>;

/// The ways a requester takes part in its own request.
#[derive(Clone, Default)]
pub struct Watch {
    progress: Option<Telling>,
    proceed: Option<Waiting>,
    closed: Option<Waiting>,
}

impl std::fmt::Debug for Watch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watch")
            .field("watched", &self.progress.is_some())
            .finish()
    }
}

impl Watch {
    /// Nobody watches: progress goes nowhere, `proceeded` is ready at once, `withdrawn` never is.
    pub fn none() -> Self {
        Self::default()
    }

    /// A watcher: `progress` carries each step to it, `proceed` resolves when it has said
    /// `Proceed`, `closed` when it has withdrawn the request.
    pub fn new<P, W, C>(progress: P, proceed: W, closed: C) -> Self
    where
        P: Fn(CallProgress) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
        W: Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
        C: Fn() -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
    {
        Self {
            progress: Some(Arc::new(progress)),
            proceed: Some(Arc::new(proceed)),
            closed: Some(Arc::new(closed)),
        }
    }

    /// A watcher that only listens: `progress` carries each step to it; it has nothing to say back
    /// (`proceeded` is ready at once, `withdrawn` never is). A `Run.Perform`'s requester.
    pub fn listening<P>(progress: P) -> Self
    where
        P: Fn(CallProgress) -> Pin<Box<dyn Future<Output = ()> + Send>> + Send + Sync + 'static,
    {
        Self {
            progress: Some(Arc::new(progress)),
            proceed: None,
            closed: None,
        }
    }

    /// Tells the watcher how far the call is.
    pub(crate) async fn tell(&self, progress: CallProgress) {
        if let Some(tell) = &self.progress {
            tell(progress).await;
        }
    }

    /// Resolves when the watcher has said `Proceed` (at once when nobody watches).
    pub(crate) async fn proceeded(&self) {
        if let Some(wait) = &self.proceed {
            wait().await;
        }
    }

    /// Resolves when the watcher withdraws the request; never when nobody watches.
    pub(crate) async fn withdrawn(&self) {
        match &self.closed {
            Some(wait) => wait().await,
            None => std::future::pending().await,
        }
    }
}

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|p| p.into_inner())
}

#[derive(Debug, Default)]
struct FlagState {
    raised: bool,
    wakers: Vec<Waker>,
}

/// A flag that is raised once and read by any number of waiters.
#[derive(Debug, Default)]
pub struct Flag(Mutex<FlagState>);

impl Flag {
    /// A flag that is down.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Raises it and wakes every waiter.
    pub fn raise(&self) {
        let mut state = locked(&self.0);
        state.raised = true;
        state.wakers.drain(..).for_each(Waker::wake);
    }

    /// Whether it is up now.
    pub fn is_up(&self) -> bool {
        locked(&self.0).raised
    }

    /// Resolves once it is up (at once if it already is).
    pub fn up(self: &Arc<Self>) -> impl Future<Output = ()> + Send + use<> {
        let flag = self.clone();
        std::future::poll_fn(move |cx| {
            let mut state = locked(&flag.0);
            if state.raised {
                Poll::Ready(())
            } else {
                state.wakers.push(cx.waker().clone());
                Poll::Pending
            }
        })
    }
}

#[derive(Debug)]
struct QueueState<T> {
    items: VecDeque<T>,
    wakers: Vec<Waker>,
}

/// Values handed from one side to the other and read one at a time, in order.
#[derive(Debug)]
pub struct Queue<T>(Mutex<QueueState<T>>);

impl<T> Default for Queue<T> {
    fn default() -> Self {
        Self(Mutex::new(QueueState {
            items: VecDeque::new(),
            wakers: Vec::new(),
        }))
    }
}

impl<T: Send> Queue<T> {
    /// An empty queue.
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Adds a value and wakes the reader.
    pub fn push(&self, item: T) {
        let mut state = locked(&self.0);
        state.items.push_back(item);
        state.wakers.drain(..).for_each(Waker::wake);
    }

    /// The next value if one is waiting.
    pub fn take(&self) -> Option<T> {
        locked(&self.0).items.pop_front()
    }

    /// The next value, waiting for one.
    pub fn next(self: &Arc<Self>) -> impl Future<Output = T> + Send + use<T> {
        let queue = self.clone();
        std::future::poll_fn(move |cx| {
            let mut state = locked(&queue.0);
            match state.items.pop_front() {
                Some(item) => Poll::Ready(item),
                None => {
                    state.wakers.push(cx.waker().clone());
                    Poll::Pending
                }
            }
        })
    }
}
