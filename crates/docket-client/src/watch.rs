//! A request the caller watches: `Request.Progress` comes out as it happens and the caller may
//! say `Proceed` or `Close` while it runs. The computer-use daemon watches its gate checks: when
//! the router is about to ask the person it says `Confirming(id)`, the daemon stops its own input
//! (so the injector cannot answer the sheet it caused), calls `proceed`, and the sheet is drawn.
//!
//! A transport that cannot watch (the default of `Transport::watch`) answers once: its events
//! are the answer alone, and `proceed` and `close` do nothing.

use crate::intents::{ClientError, Intents};
use crate::transport::{Transport, TransportError};
use docket_core::{
    ActivationToken, CallProgress, CallRefusal, CallRequest, ConfirmId, CuaAsk, GateAnswer,
    IntentsReply, IntentsRequest, Outcome, WindowKey,
};
use prov::SessionId;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

/// A boxed future, so the two ends of a watch are trait objects.
pub type Boxed<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// One thing a watched request says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Said {
    /// How far it is.
    Progress(CallProgress),
    /// Its answer. Nothing follows.
    Answer(Box<IntentsReply>),
}

/// What a watched request says, in order.
pub trait Events: Send {
    /// The next thing it says: progress, then the answer, then `Closed`.
    fn next(&mut self) -> Boxed<'_, Result<Said, TransportError>>;
}

/// What the caller says back to a request in flight.
pub trait Steering: Send + Sync {
    /// The caller has suspended what it holds (its input lease): the router may show the sheet.
    fn proceed(&self) -> Boxed<'_, Result<(), TransportError>>;
    /// Takes the request back: no answer follows, and a sheet that is up is withdrawn.
    fn close(&self) -> Boxed<'_, Result<(), TransportError>>;
    /// The request's object path on the bus (`/org/quire/Intents1/request/<n>`), for a transport
    /// that has one; none for the router in process.
    fn request(&self) -> Option<String> {
        None
    }
}

/// The two ends of one watched request.
pub struct Watched {
    /// What it says.
    pub events: Box<dyn Events>,
    /// What may be said to it.
    pub steering: Arc<dyn Steering>,
}

impl std::fmt::Debug for Watched {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Watched")
    }
}

/// A request answered in one piece: its events are the answer alone.
#[derive(Debug)]
struct Once(Option<IntentsReply>);

impl Events for Once {
    fn next(&mut self) -> Boxed<'_, Result<Said, TransportError>> {
        let said = self.0.take().map(|reply| Said::Answer(Box::new(reply)));
        Box::pin(std::future::ready(said.ok_or(TransportError::Closed)))
    }
}

/// Nothing to proceed or close: the request is already answered.
#[derive(Debug)]
struct Settled;

impl Steering for Settled {
    fn proceed(&self) -> Boxed<'_, Result<(), TransportError>> {
        Box::pin(std::future::ready(Ok(())))
    }

    fn close(&self) -> Boxed<'_, Result<(), TransportError>> {
        Box::pin(std::future::ready(Ok(())))
    }
}

impl Watched {
    /// A request that was answered in one piece.
    pub fn answered(reply: IntentsReply) -> Self {
        Self {
            events: Box::new(Once(Some(reply))),
            steering: Arc::new(Settled),
        }
    }
}

/// What a watched gate check says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GateEvent {
    /// The router is about to ask the person about this step (sheet `id`): suspend the run's
    /// own input, then `proceed`.
    Confirming(ConfirmId),
    /// The step's verdict. Nothing follows.
    Verdict(GateAnswer),
}

/// The caller's hand on a gate check in flight; cheap to clone and send to another task.
#[derive(Clone)]
pub struct GateSteer(Arc<dyn Steering>);

impl std::fmt::Debug for GateSteer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GateSteer")
    }
}

impl GateSteer {
    /// The run's input is suspended: the router may show the sheet.
    pub async fn proceed(&self) -> Result<(), ClientError> {
        Ok(self.0.proceed().await?)
    }

    /// Withdraws the check (a paused run): the sheet comes down and no verdict follows.
    pub async fn close(&self) -> Result<(), ClientError> {
        Ok(self.0.close().await?)
    }
}

/// A gate check in flight: read its events, steer it.
pub struct GateWatch {
    events: Box<dyn Events>,
    steer: GateSteer,
}

impl std::fmt::Debug for GateWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GateWatch")
    }
}

impl GateWatch {
    /// The next event. Other progress is not the gate's and is skipped; a verdict ends the
    /// watch, and a refused request is an error.
    pub async fn next(&mut self) -> Result<GateEvent, ClientError> {
        loop {
            match self.events.next().await? {
                Said::Progress(CallProgress::Confirming(id)) => {
                    return Ok(GateEvent::Confirming(id));
                }
                Said::Progress(_) => {}
                Said::Answer(reply) => {
                    return match *reply {
                        IntentsReply::Gate(answer) => Ok(GateEvent::Verdict(answer)),
                        IntentsReply::Refused(why) => Err(ClientError::Refused(why)),
                        _ => Err(ClientError::Unexpected),
                    };
                }
            }
        }
    }

    /// A hand on the check that another task may hold while this one reads the events.
    pub fn steer(&self) -> GateSteer {
        self.steer.clone()
    }

    /// `GateSteer::proceed`.
    pub async fn proceed(&self) -> Result<(), ClientError> {
        self.steer.proceed().await
    }

    /// `GateSteer::close`.
    pub async fn close(&self) -> Result<(), ClientError> {
        self.steer.close().await
    }
}

impl<T: Transport> Intents<T> {
    /// Computer use: may this pixel step run, watched. The router says `Confirming(id)` before it
    /// draws a sheet and waits for `proceed`; the verdict is the last event. A transport that
    /// cannot watch gives the verdict alone (`gate_check` is the same without the watch).
    pub async fn gate_check_watched(&self, ask: CuaAsk) -> Result<GateWatch, ClientError> {
        let Watched { events, steering } = self
            .transport()
            .watch(IntentsRequest::GateCheck(ask))
            .await?;
        Ok(GateWatch {
            events,
            steer: GateSteer(steering),
        })
    }
}

/// What a watched `Run.Perform` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerformEvent {
    /// How far the call is: `Reviewing`, `Previewing`, `Confirming(id)` (the sheet is up),
    /// `Dispatched`.
    Progress(CallProgress),
    /// The call's own end. Nothing follows.
    Done(Box<Result<Outcome, CallRefusal>>),
}

/// A `Run.Perform` in flight: read what it says. Nothing waits for the caller.
pub struct PerformWatch {
    events: Box<dyn Events>,
    request: Option<String>,
}

impl std::fmt::Debug for PerformWatch {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PerformWatch")
    }
}

impl PerformWatch {
    /// The request's object path on the bus, for a transport that has one (a call over D-Bus);
    /// none for the router in process.
    pub fn request(&self) -> Option<&str> {
        self.request.as_deref()
    }

    /// The next event; the call's end is the last, and a refused request is an error.
    pub async fn next(&mut self) -> Result<PerformEvent, ClientError> {
        match self.events.next().await? {
            Said::Progress(progress) => Ok(PerformEvent::Progress(progress)),
            Said::Answer(reply) => match *reply {
                IntentsReply::Performed(end) => Ok(PerformEvent::Done(end)),
                IntentsReply::Refused(why) => Err(ClientError::Refused(why)),
                _ => Err(ClientError::Unexpected),
            },
        }
    }
}

impl<T: Transport> Intents<T> {
    /// `perform`, watched: the progress of the call comes out as it happens (a person's sheet is
    /// announced as `Confirming(id)` as it is drawn) and the end is the last event. A transport
    /// that cannot watch gives the end alone.
    pub async fn perform_watched(
        &self,
        call: CallRequest,
        session: Option<SessionId>,
        parent_window: Option<WindowKey>,
    ) -> Result<PerformWatch, ClientError> {
        self.perform_watched_activated(call, session, parent_window, None)
            .await
    }

    /// `perform_watched` with the launcher's activation token (see `perform_activated`).
    pub async fn perform_watched_activated(
        &self,
        call: CallRequest,
        session: Option<SessionId>,
        parent_window: Option<WindowKey>,
        activation: Option<ActivationToken>,
    ) -> Result<PerformWatch, ClientError> {
        let Watched { events, steering } = self
            .transport()
            .watch(IntentsRequest::Perform {
                activation,
                call,
                session,
                parent_window,
            })
            .await?;
        Ok(PerformWatch {
            events,
            request: steering.request(),
        })
    }
}
