//! Watching a request of the router in the caller's own process. There is no runtime here to
//! run the router's future beside the caller, so the events drive it: the future is polled by
//! `Events::next`, which hands out what it said (progress, then the answer) in order. A caller
//! that proceeds or closes must keep reading the events for the router to see it.

use crate::transport::TransportError;
use crate::watch::{Boxed, Events, Said, Steering, Watched};
use docket_core::{CallProgress, CallerId, IntentsReply, IntentsRequest};
use docket_router::{Flag, Queue, Router, Seams, Watch};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::Poll;

type Handling = Pin<Box<dyn Future<Output = IntentsReply> + Send>>;

struct Driven {
    handling: Option<Handling>,
    answer: Option<IntentsReply>,
    progress: Arc<Queue<CallProgress>>,
}

impl Events for Driven {
    fn next(&mut self) -> Boxed<'_, Result<Said, TransportError>> {
        Box::pin(std::future::poll_fn(move |cx| {
            if let Some(handling) = self.handling.as_mut()
                && let Poll::Ready(reply) = handling.as_mut().poll(cx)
            {
                self.handling = None;
                self.answer = Some(reply);
            }
            if let Some(progress) = self.progress.take() {
                return Poll::Ready(Ok(Said::Progress(progress)));
            }
            match (self.answer.take(), &self.handling) {
                (Some(reply), _) => Poll::Ready(Ok(Said::Answer(reply))),
                (None, None) => Poll::Ready(Err(TransportError::Closed)),
                (None, Some(_)) => Poll::Pending,
            }
        }))
    }
}

struct Flags {
    proceed: Arc<Flag>,
    closed: Arc<Flag>,
}

impl Steering for Flags {
    fn proceed(&self) -> Boxed<'_, Result<(), TransportError>> {
        self.proceed.raise();
        Box::pin(std::future::ready(Ok(())))
    }

    fn close(&self) -> Boxed<'_, Result<(), TransportError>> {
        self.closed.raise();
        Box::pin(std::future::ready(Ok(())))
    }
}

/// `request` handled by `router` as `caller`, watched.
pub(crate) fn watched<S: Seams + 'static>(
    router: Arc<Router<S>>,
    caller: CallerId,
    request: IntentsRequest,
) -> Watched {
    let progress = Queue::new();
    let (proceed, closed) = (Flag::new(), Flag::new());
    let (told, waited, withdrawn) = (progress.clone(), proceed.clone(), closed.clone());
    let watch = Watch::new(
        move |p| {
            told.push(p);
            Box::pin(std::future::ready(()))
        },
        move || Box::pin(waited.up()),
        move || Box::pin(withdrawn.up()),
    );
    let handling: Handling =
        Box::pin(async move { router.handle_watched(&caller, request, watch).await });
    Watched {
        events: Box::new(Driven {
            handling: Some(handling),
            answer: None,
            progress,
        }),
        steering: Arc::new(Flags { proceed, closed }),
    }
}
