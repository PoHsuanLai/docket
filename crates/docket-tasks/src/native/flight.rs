//! A turn in flight: the drive loop as a future the backend keeps, and the channels it talks
//! through. Pulling an event polls the kept future and reads its channel; nothing that was
//! produced is held by a pull's own future, so dropping a pull at any point loses nothing and
//! the next pull goes on from the same place (the cancel-safety contract of `next_event`).
//!
//! A call waits at the loop's gate after its `Started` event is queued. The backend lets it go
//! on the pull that comes after the one that returned `Started`, and not before: whoever reads
//! the events (an editor's permission round) has the whole time between the two pulls to stop it.

use crate::tap::{Go, Tap};
use companion_wire::NeedsYou;
use docket_core::{Reveal, StepLine};
use docket_session::{BackendEvent, CallEvent, CallOpen, TurnEnd};
use futures_channel::mpsc::{UnboundedReceiver, UnboundedSender, unbounded};
use futures_util::StreamExt;
use futures_util::future::{BoxFuture, Either, select};

/// Whether the gate still lets calls through.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Gate {
    Open,
    Stopped,
}

/// The loop's side: what it tells goes down `events`, and a call waits on `release`.
#[derive(Debug)]
pub(crate) struct ChannelTap {
    events: UnboundedSender<BackendEvent>,
    release: UnboundedReceiver<Go>,
    gate: Gate,
}

impl ChannelTap {
    fn tell(&self, event: BackendEvent) {
        // A reader that went away has no one left to tell.
        let _ = self.events.unbounded_send(event);
    }
}

impl Tap for ChannelTap {
    fn said(&mut self, words: &str) {
        self.tell(BackendEvent::Words(Reveal::Plain(words.to_owned())));
    }

    fn needs(&mut self, need: &NeedsYou) {
        self.tell(BackendEvent::NeedsYou(need.clone()));
    }

    async fn gate(&mut self, open: CallOpen) -> Go {
        if self.gate == Gate::Stopped {
            return Go::Stop;
        }
        self.tell(BackendEvent::Call(CallEvent::Started(open)));
        match self.release.next().await {
            Some(Go::Run) => Go::Run,
            Some(Go::Stop) | None => {
                self.gate = Gate::Stopped;
                Go::Stop
            }
        }
    }

    fn ended(&mut self, line: &StepLine) {
        self.tell(BackendEvent::Call(CallEvent::Ended(line.clone())));
    }
}

/// What the reader owes the loop for the last event it was given.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Owed {
    /// Nothing.
    Nothing,
    /// A call was announced: it goes on at the next pull.
    Release,
}

/// What woke a pull.
enum Woke {
    Ended(TurnEnd),
    Event(Option<BackendEvent>),
}

/// The reader's side of a turn in flight.
pub(crate) struct Flight {
    driver: Option<BoxFuture<'static, TurnEnd>>,
    events: UnboundedReceiver<BackendEvent>,
    release: UnboundedSender<Go>,
    owed: Owed,
    ended: Option<TurnEnd>,
}

impl std::fmt::Debug for Flight {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Flight({:?})", self.owed)
    }
}

impl Flight {
    /// The two ends of a turn: the tap the loop is given and the flight that reads it. `drive`
    /// makes the future that runs the loop from the tap.
    pub(crate) fn of(drive: impl FnOnce(ChannelTap) -> BoxFuture<'static, TurnEnd>) -> Flight {
        let (events_tx, events) = unbounded();
        let (release, release_rx) = unbounded();
        let tap = ChannelTap {
            events: events_tx,
            release: release_rx,
            gate: Gate::Open,
        };
        Flight {
            driver: Some(drive(tap)),
            events,
            release,
            owed: Owed::Nothing,
            ended: None,
        }
    }

    /// Stops the turn: a call waiting at the gate is never made. A turn that is between calls
    /// ends at the loop's next step, which the caller asks for with the cancel flag.
    pub(crate) fn stop(&mut self) {
        self.owed = Owed::Nothing;
        let _ = self.release.unbounded_send(Go::Stop);
    }

    /// The next event of the turn; the turn's end last, then `None`. Cancel-safe.
    pub(crate) async fn next(&mut self) -> Option<BackendEvent> {
        if self.owed == Owed::Release {
            self.owed = Owed::Nothing;
            let _ = self.release.unbounded_send(Go::Run);
        }
        loop {
            if let Ok(event) = self.events.try_recv() {
                return Some(self.seen(event));
            }
            let Some(driver) = self.driver.as_mut() else {
                return self.ended.take().map(BackendEvent::TurnEnd);
            };
            let woke = match select(driver, self.events.next()).await {
                Either::Left((end, _)) => Woke::Ended(end),
                Either::Right((event, _)) => Woke::Event(event),
            };
            match woke {
                Woke::Ended(end) => {
                    self.driver = None;
                    self.ended = Some(end);
                }
                Woke::Event(Some(event)) => return Some(self.seen(event)),
                // The loop dropped its tap, which it does as it ends: only its end is left.
                Woke::Event(None) => {
                    if let Some(driver) = self.driver.as_mut() {
                        let end = driver.await;
                        self.driver = None;
                        self.ended = Some(end);
                    }
                }
            }
        }
    }

    fn seen(&mut self, event: BackendEvent) -> BackendEvent {
        if matches!(event, BackendEvent::Call(CallEvent::Started(_))) {
            self.owed = Owed::Release;
        }
        event
    }
}
