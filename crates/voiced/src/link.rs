//! The inferd side: one session per turn kind, run by its own task so a cold engine never stops
//! the daemon's loop. The loop writes `ClientFrame`s into the task's inbox and reads
//! `LinkEvent`s out of its outbox. Dropping the inbox ends the task and, with it, the session.
//! The session's `next` must be cancel safe (porter's framed sessions say so).

use porter_client::{InferSession, Transport, TransportError};
use porter_core::{DataClass, Need, Tier};
use porter_infer::{ClientFrame, InferEvent, InferRefusal, ModelError};
use std::sync::Arc;
use tokio::sync::mpsc;
use voice_wire::VoiceFault;

/// What the task tells the loop.
#[derive(Debug)]
pub(crate) enum LinkEvent {
    /// The session is open and its first frame is sent: audio may flow.
    Ready,
    /// inferd said something.
    Infer(Box<InferEvent>),
    /// The session could not open or died.
    Closed(VoiceFault),
}

/// The loop's end of a session task.
#[derive(Debug)]
pub(crate) struct Link {
    pub tx: mpsc::UnboundedSender<ClientFrame>,
    pub rx: mpsc::UnboundedReceiver<LinkEvent>,
}

/// How a failed open reads to the person.
pub(crate) fn fault_of(error: &TransportError) -> VoiceFault {
    match error {
        TransportError::Denied(_) => VoiceFault::Refused(InferRefusal::Denied),
        TransportError::Unreachable => VoiceFault::Engine(ModelError::Unreachable),
        _ => VoiceFault::Engine(ModelError::Unreadable),
    }
}

/// Opens a session for `need` and writes `first` to it, then relays frames and events.
pub(crate) fn spawn<T: Transport + 'static>(
    transport: Arc<T>,
    need: Need,
    class: DataClass,
    tier: Tier,
    first: ClientFrame,
) -> Link
where
    T::Session: 'static,
{
    let (tx, mut inbox) = mpsc::unbounded_channel::<ClientFrame>();
    let (outbox, rx) = mpsc::unbounded_channel::<LinkEvent>();
    tokio::spawn(async move {
        let mut session = match transport.open(&need, class, tier).await {
            Ok(session) => session,
            Err(error) => {
                let _ = outbox.send(LinkEvent::Closed(fault_of(&error)));
                return;
            }
        };
        if session.send(first).await.is_err() {
            let _ = outbox.send(LinkEvent::Closed(VoiceFault::Engine(
                ModelError::Unreachable,
            )));
            return;
        }
        let _ = outbox.send(LinkEvent::Ready);
        loop {
            tokio::select! {
                frame = inbox.recv() => {
                    let Some(frame) = frame else { return };
                    if session.send(frame).await.is_err() {
                        let _ = outbox.send(LinkEvent::Closed(VoiceFault::Engine(
                            ModelError::Unreachable,
                        )));
                        return;
                    }
                }
                event = session.next() => match event {
                    Ok(event) => {
                        if outbox.send(LinkEvent::Infer(Box::new(event))).is_err() {
                            return;
                        }
                    }
                    Err(_) => {
                        let _ = outbox.send(LinkEvent::Closed(VoiceFault::Engine(
                            ModelError::Unreachable,
                        )));
                        return;
                    }
                },
            }
        }
    });
    Link { tx, rx }
}
