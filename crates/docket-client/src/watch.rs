//! A request the caller watches: `Request.Progress` comes out as it happens and the caller may
//! say `Proceed` or `Close` while it runs. The computer-use daemon watches its gate checks: when
//! the router is about to ask the person it says `Confirming(id)`, the daemon stops its own input
//! (so the injector cannot answer the sheet it caused), calls `proceed`, and the sheet is drawn.
//!
//! A transport that cannot watch (the default of `Transport::watch`) answers once: its events
//! are the answer alone, and `proceed` and `close` do nothing.

use crate::intents::{ClientError, Intents};
use crate::transport::{Transport, TransportError};
use docket_core::{CallProgress, ConfirmId, CuaAsk, GateAnswer, IntentsReply, IntentsRequest};
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
