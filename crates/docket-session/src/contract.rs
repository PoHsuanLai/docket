//! For tests: the rules every `SessionBackend` keeps, written once and run against each backend
//! (the scripted `FakeBackend` here, the native backend in `docket-tasks`). A backend's test
//! implements [`Harness`]; the functions below drive it with no executor of their own, so they
//! run on whatever the caller's test runs on.
//!
//! The turn they run makes one call and then says words: `Started`, `Ended`, `Words`, then
//! `TurnEnd(Done)`.

use crate::backend::{BackendEvent, BackendFault, CallEvent, SessionBackend, TurnEnd};
use docket_core::{CallId, Reveal, UserTurn};
use std::future::Future;
use std::pin::pin;
use std::task::{Context, Poll, Waker};

/// What the contract needs of the backend under test.
pub trait Harness {
    /// The backend.
    type Backend: SessionBackend;

    /// A backend that is started and idle, whose next turn is the contract's turn.
    fn backend(&mut self) -> impl Future<Output = Self::Backend>;

    /// A turn to give it.
    fn turn(&self, n: u64) -> UserTurn;

    /// How many times the call has actually been made (an app saw it), as of now.
    fn ran(&self, backend: &Self::Backend) -> usize;

    /// Lets everything that can run, run: a backend that makes calls on its own would have by
    /// the time this returns.
    fn settle(&mut self) -> impl Future<Output = ()>;

    /// How many pulls of one turn are at least expected to be pending on first poll, so that a
    /// test of dropped pulls is not vacuous. A backend that never waits says 0.
    fn stalls(&self) -> usize;
}

/// An event with what varies between backends taken out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// A call was announced.
    Started(CallId),
    /// A call ended.
    Ended(CallId),
    /// Words.
    Words,
    /// The turn ended.
    End(TurnEnd),
    /// Anything else.
    Other,
}

/// `event` without its content.
pub fn shape(event: &BackendEvent) -> Shape {
    match event {
        BackendEvent::Call(CallEvent::Started(open)) => Shape::Started(open.call),
        BackendEvent::Call(CallEvent::Ended(step)) => Shape::Ended(step.call),
        BackendEvent::Words(Reveal::Plain(_)) => Shape::Words,
        BackendEvent::TurnEnd(end) => Shape::End(*end),
        _ => Shape::Other,
    }
}

/// Polls `future` once with a waker that does nothing and drops it if it is not ready: a pull
/// given up on.
fn once<F: Future>(future: F) -> Option<F::Output> {
    let mut future = pin!(future);
    let mut cx = Context::from_waker(Waker::noop());
    match future.as_mut().poll(&mut cx) {
        Poll::Ready(out) => Some(out),
        Poll::Pending => None,
    }
}

/// Pulls to the end of the turn, shapes in order.
async fn drain<B: SessionBackend>(backend: &mut B) -> Vec<Shape> {
    let mut shapes = Vec::new();
    while let Some(event) = backend.next_event().await {
        shapes.push(shape(&event));
    }
    shapes
}

/// The one turn the contract runs, as shapes, and what it left.
async fn whole_turn<H: Harness>(h: &mut H) -> (Vec<Shape>, usize) {
    let mut backend = h.backend().await;
    backend.turn(h.turn(1)).await.expect("turn");
    let shapes = drain(&mut backend).await;
    (shapes, h.ran(&backend))
}

/// A turn's events are `Started`, `Ended` (the same call), `Words`, and the end last; the call ran
/// once; the backend then takes another turn; and a second turn while one answers is `Busy`.
pub async fn runs_a_turn<H: Harness>(h: &mut H) {
    let (shapes, ran) = whole_turn(h).await;
    let call = match shapes.first() {
        Some(Shape::Started(call)) => *call,
        other => panic!("the turn begins by announcing its call: {other:?}"),
    };
    assert_eq!(
        shapes,
        [
            Shape::Started(call),
            Shape::Ended(call),
            Shape::Words,
            Shape::End(TurnEnd::Done)
        ]
    );
    assert_eq!(ran, 1, "the call ran once");

    let mut backend = h.backend().await;
    backend.turn(h.turn(1)).await.expect("turn");
    assert_eq!(
        backend.turn(h.turn(2)).await,
        Err(BackendFault::Busy),
        "one turn at a time"
    );
    drain(&mut backend).await;
    assert_eq!(backend.next_event().await, None, "nothing after the end");
    backend.turn(h.turn(2)).await.expect("a turn after the end");
    drain(&mut backend).await;
}

/// A stop between a call's announcement and the pull after it means the call never runs: the
/// turn ends `Cancelled` and no `Ended` follows.
pub async fn a_stop_before_the_next_pull_stops_the_call<H: Harness>(h: &mut H) {
    let mut backend = h.backend().await;
    backend.turn(h.turn(1)).await.expect("turn");
    let first = backend.next_event().await.expect("an event");
    assert!(matches!(shape(&first), Shape::Started(_)), "{first:?}");
    backend.cancel().await;
    h.settle().await;
    let rest = drain(&mut backend).await;
    assert_eq!(
        rest.last(),
        Some(&Shape::End(TurnEnd::Cancelled)),
        "{rest:?}"
    );
    assert!(
        !rest.iter().any(|s| matches!(s, Shape::Ended(_))),
        "a call that never ran has no end: {rest:?}"
    );
    assert_eq!(h.ran(&backend), 0, "the call was never made");
}

/// A call does not run until the pull after the one that returned its announcement, however long
/// the reader takes.
pub async fn a_call_waits_for_the_pull_after_its_announcement<H: Harness>(h: &mut H) {
    let mut backend = h.backend().await;
    backend.turn(h.turn(1)).await.expect("turn");
    let first = backend.next_event().await.expect("an event");
    assert!(matches!(shape(&first), Shape::Started(_)), "{first:?}");
    h.settle().await;
    assert_eq!(h.ran(&backend), 0, "announced, not yet allowed to run");
    let next = backend.next_event().await.expect("an event");
    assert!(matches!(shape(&next), Shape::Ended(_)), "{next:?}");
    assert_eq!(h.ran(&backend), 1);
    drain(&mut backend).await;
}

/// Dropping a pull before it is ready loses no event and repeats none: the same turn pulled with
/// every pull given up on first, once, ends with the same events and the same effect.
pub async fn a_dropped_pull_loses_and_repeats_nothing<H: Harness>(h: &mut H) {
    let (whole, ran) = whole_turn(h).await;

    let mut backend = h.backend().await;
    backend.turn(h.turn(1)).await.expect("turn");
    let (mut shapes, mut given_up) = (Vec::new(), 0usize);
    loop {
        let polled = once(backend.next_event());
        let event = match polled {
            Some(event) => event,
            None => {
                given_up += 1;
                backend.next_event().await
            }
        };
        match event {
            Some(event) => shapes.push(shape(&event)),
            None => break,
        }
    }
    assert!(
        given_up >= h.stalls(),
        "the test gave up on {given_up} pulls, expected at least {}",
        h.stalls()
    );
    assert_eq!(shapes, whole);
    assert_eq!(h.ran(&backend), ran);
}
