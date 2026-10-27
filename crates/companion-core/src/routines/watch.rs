//! Watching the almanac for the events routines wait for: when to ask again, which Space to ask
//! about which kinds, which events were already acted on, and which routines an event is for.
//! Asking is the host's; this is what is asked and what the answers mean.

use almanac_core::{EventRef, KindPattern};
use docket_core::routine::{EventMatch, RoutineBook, RoutineId};
use prov::{HelperId, SpaceId, UnixSeconds};
use std::collections::VecDeque;

/// How often the almanac is asked for events, in seconds.
const EVENTS_EVERY: i64 = 30;
/// How far back a question to the almanac reaches past the last one, in seconds, so an event on the
/// boundary is not lost (events are told apart by their handle).
const OVERLAP: i64 = 2;
/// The most event handles remembered.
const MOST_SEEN: usize = 512;

/// A question to the almanac: which events of these kinds happened in a Space since a time.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventAsk {
    /// The Space.
    pub space: SpaceId,
    /// From when.
    pub since: UnixSeconds,
    /// Which kinds.
    pub kinds: Vec<KindPattern>,
}

/// An event the almanac answered with: its handle and its kind, nothing of its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The event.
    pub event: EventRef,
    /// Its kind.
    pub kind: String,
}

/// When the almanac was last asked, and which events have been acted on.
#[derive(Debug)]
pub struct EventWatch {
    polled: UnixSeconds,
    seen: VecDeque<EventRef>,
}

impl EventWatch {
    /// A watch that has just asked, at `now`.
    pub fn new(now: UnixSeconds) -> Self {
        Self {
            polled: now,
            seen: VecDeque::new(),
        }
    }

    /// Events from before `now` are not acted on.
    pub fn restart(&mut self, now: UnixSeconds) {
        self.polled = now;
    }

    /// Whether it is time to ask again. If so, the time of the last question, which this one
    /// follows, and now counts as the last.
    pub fn due(&mut self, now: UnixSeconds) -> Option<UnixSeconds> {
        if now.0 - self.polled.0 < EVENTS_EVERY {
            return None;
        }
        Some(std::mem::replace(&mut self.polled, now))
    }

    /// Whether `event` is new; it is remembered.
    pub fn remember(&mut self, event: &EventRef) -> bool {
        if self.seen.contains(event) {
            return false;
        }
        self.seen.push_back(event.clone());
        while self.seen.len() > MOST_SEEN {
            self.seen.pop_front();
        }
        true
    }
}

/// One Space to ask about, and the routines that watch in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpaceWatch {
    /// The question.
    pub ask: EventAsk,
    /// The routines whose events it asks for.
    pub routines: Vec<(RoutineId, EventMatch)>,
}

impl SpaceWatch {
    /// The routines `seen` is an event for.
    pub fn covering(&self, seen: &Seen) -> Vec<RoutineId> {
        self.routines
            .iter()
            .filter(|(_, watch)| watch.covers(&seen.kind, &seen.event.space))
            .map(|(routine, _)| *routine)
            .collect()
    }
}

/// What to ask the almanac since `since`: one question per Space (Spaces are walls), for the
/// events the book's watching routines wait for. A routine watches its helper's Space, or the one
/// it names; `space_of` tells a helper's Space, and a routine whose helper is gone asks nothing.
pub fn event_asks(
    book: &RoutineBook,
    since: UnixSeconds,
    space_of: impl Fn(&HelperId) -> Option<SpaceId>,
) -> Vec<SpaceWatch> {
    let watches = book.watches().into_iter().filter_map(|(routine, event)| {
        let space = event.space.clone().or_else(|| space_of(&routine.helper))?;
        Some((space, (routine.id, event.clone())))
    });
    crate::records::group_by_space(watches)
        .into_iter()
        .map(|(space, routines)| SpaceWatch {
            ask: EventAsk {
                space,
                since: UnixSeconds(since.0 - OVERLAP),
                kinds: routines
                    .iter()
                    .map(|(_, event)| event.kind.clone())
                    .collect(),
            },
            routines,
        })
        .collect()
}
