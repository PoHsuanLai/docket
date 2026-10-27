//! The scheduler: the routine book's owner. A small timer inside companiond (not a daemon of its
//! own) ticks every few seconds; each tick asks which runs ended, which times are due, and what
//! the almanac saw. A time that came while the computer slept or the assistant was down shows as a
//! late tick, and the book skips it (or runs it once, when the person said so). Systemd user
//! timers, written from the same schedules, only start companiond at the time.

use super::host::{Awake, EventAsk, Host, Seen};
use super::units::units_of;
use almanac_core::EventRef;
use docket_core::Clock;
use docket_core::routine::{
    EventMatch, JobEnd, RoutineBook, RoutineEffect, RoutineEvent, RoutineId, RoutineRecord,
    TimerEntry, step,
};
use prov::{SpaceId, UnixSeconds};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the timer ticks.
const POLL: Duration = Duration::from_secs(5);
/// How often the almanac is asked for events, in seconds.
const EVENTS_EVERY: i64 = 30;
/// How far back a question to the almanac reaches past the last one, in seconds, so an event on the
/// boundary is not lost (events are told apart by their handle).
const OVERLAP: i64 = 2;
/// The most event handles remembered.
const MOST_SEEN: usize = 512;

struct State {
    book: RoutineBook,
    /// The timers last installed; none before the first time.
    installed: Option<Vec<TimerEntry>>,
    awake: Awake,
    /// Events already acted on.
    seen: VecDeque<EventRef>,
    /// When the almanac was last asked.
    polled: UnixSeconds,
}

/// The routines, over a [`Host`].
pub struct Routines<H: Host, K: Clock> {
    host: Arc<H>,
    clock: K,
    state: Mutex<State>,
}

impl<H: Host, K: Clock> fmt::Debug for Routines<H, K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Routines").finish_non_exhaustive()
    }
}

impl<H: Host, K: Clock> Routines<H, K> {
    /// No routines yet; `restore` brings the stored ones back.
    pub fn new(host: Arc<H>, clock: K) -> Self {
        let polled = clock.now();
        Self {
            host,
            clock,
            state: Mutex::new(State {
                book: RoutineBook::default(),
                installed: None,
                awake: Awake::NotNeeded,
                seen: VecDeque::new(),
                polled,
            }),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.state.lock() {
            Ok(mut state) => f(&mut state),
            Err(poisoned) => f(&mut poisoned.into_inner()),
        }
    }

    /// The book as it stands.
    pub fn book(&self) -> RoutineBook {
        self.with(|state| state.book.clone())
    }

    pub(super) fn now(&self) -> UnixSeconds {
        self.clock.now()
    }

    pub(super) fn host(&self) -> &H {
        &self.host
    }

    /// Brings the routines back from the records the Spaces hold. A routine whose helper is gone
    /// is left out. Events from before now are not acted on.
    pub async fn restore(&self, spaces: &[SpaceId]) {
        let records = self.host.read(spaces).await;
        let kept: Vec<RoutineRecord> = RoutineBook::restore(records)
            .records()
            .into_iter()
            .filter(|record| self.host.space_of(record.helper()).is_some())
            .collect();
        let now = self.clock.now();
        self.with(|state| {
            state.book = RoutineBook::restore(kept);
            state.polled = now;
        });
        self.settle();
    }

    /// Moves the book by `event` and carries out what comes of it, and of what that sets off. The
    /// id of a routine this made, if it made one.
    pub async fn apply(&self, event: RoutineEvent) -> Option<RoutineId> {
        let mut made = None;
        let mut todo = VecDeque::from([event]);
        while let Some(next) = todo.pop_front() {
            let effects = self.step(next);
            made = made.or(effects.iter().find_map(|effect| match effect {
                RoutineEffect::Created(id) => Some(*id),
                _ => None,
            }));
            todo.extend(self.carry_out(effects).await);
        }
        made
    }

    fn step(&self, event: RoutineEvent) -> Vec<RoutineEffect> {
        self.with(|state| {
            let (book, effects) = step(std::mem::take(&mut state.book), event);
            state.book = book;
            effects
        })
    }

    /// Carries the effects out; what fell out of them (a posted job) goes back to the book.
    async fn carry_out(&self, effects: Vec<RoutineEffect>) -> Vec<RoutineEvent> {
        let mut follow = Vec::new();
        let mut saved = Vec::new();
        for effect in effects {
            match effect {
                RoutineEffect::Save(record) => saved.push(record),
                RoutineEffect::Created(_) => {}
                RoutineEffect::Say { helper, notice } => {
                    if let Some(name) = self.host.name_of(&helper) {
                        self.host.say(&helper, notice.plain(&name));
                    }
                }
                RoutineEffect::Run(run) => {
                    let routine = run.routine;
                    follow.push(match self.host.post(run).await {
                        Some(job) => RoutineEvent::Posted { routine, job },
                        None => RoutineEvent::NotPosted(routine),
                    });
                }
            }
        }
        self.write(saved).await;
        self.settle();
        follow
    }

    /// Stores `records`, each in the Space of its helper.
    async fn write(&self, records: Vec<RoutineRecord>) {
        let mut groups: Vec<(SpaceId, Vec<RoutineRecord>)> = Vec::new();
        for record in records {
            let Some(space) = self.host.space_of(record.helper()) else {
                continue;
            };
            match groups.iter_mut().find(|(held, _)| *held == space) {
                Some((_, held)) => held.push(record),
                None => groups.push((space, vec![record])),
            }
        }
        for (space, records) in groups {
            self.host.save(&space, records).await;
        }
    }

    /// Tells the host what changed since it was last told: the timers wanted, and whether the
    /// desktop must stay awake (only while a run is in progress).
    fn settle(&self) {
        let (units, awake) = self.with(|state| {
            let timers = state.book.timers();
            let units = (state.installed.as_ref() != Some(&timers)).then(|| units_of(&timers));
            if units.is_some() {
                state.installed = Some(timers);
            }
            let need = match state.book.running() {
                0 => Awake::NotNeeded,
                _ => Awake::Needed,
            };
            let awake = (state.awake != need).then_some(need);
            state.awake = need;
            (units, awake)
        });
        if let Some(units) = units {
            self.host.install(&units);
        }
        if let Some(need) = awake {
            self.host.awake(need);
        }
    }

    /// One tick: the runs that ended, the times that are due, and the events the almanac saw.
    pub async fn tick(&self) {
        let now = self.clock.now();
        let (running, awaited) =
            self.with(|state| (state.book.running_jobs(), state.book.awaited_jobs()));
        for job in running {
            if let Some(end) = self.host.job_end(job) {
                self.apply(RoutineEvent::Ended { job, end, now }).await;
            }
        }
        // A job a routine waits for starts it only by ending done.
        for job in awaited {
            if self.host.job_end(job) == Some(JobEnd::Done) {
                self.apply(RoutineEvent::Ended {
                    job,
                    end: JobEnd::Done,
                    now,
                })
                .await;
            }
        }
        let zone = self.host.zone(now);
        self.apply(RoutineEvent::Tick { now, zone }).await;
        self.poll_events(now).await;
    }

    /// Ticks until the daemon ends. The first tick comes at once, so a time that passed while the
    /// assistant was down is seen (and skipped) as soon as it starts.
    pub async fn run(self: Arc<Self>) {
        loop {
            self.tick().await;
            tokio::time::sleep(POLL).await;
        }
    }

    /// Whether `event` is new; it is remembered.
    fn remember(&self, event: &EventRef) -> bool {
        self.with(|state| {
            if state.seen.contains(event) {
                return false;
            }
            state.seen.push_back(event.clone());
            while state.seen.len() > MOST_SEEN {
                state.seen.pop_front();
            }
            true
        })
    }

    async fn poll_events(&self, now: UnixSeconds) {
        let plan = self.with(|state| {
            if now.0 - state.polled.0 < EVENTS_EVERY {
                return None;
            }
            let since = state.polled;
            state.polled = now;
            let watches: Vec<(RoutineId, prov::HelperId, EventMatch)> = state
                .book
                .watches()
                .into_iter()
                .map(|(routine, event)| (routine.id, routine.helper.clone(), event.clone()))
                .collect();
            Some((since, watches))
        });
        let Some((since, watches)) = plan else {
            return;
        };
        // Spaces are walls: a routine watches its helper's Space, or the one it names.
        let mut by_space: Vec<(SpaceId, Vec<(RoutineId, EventMatch)>)> = Vec::new();
        for (routine, helper, event) in watches {
            let Some(space) = event.space.clone().or_else(|| self.host.space_of(&helper)) else {
                continue;
            };
            match by_space.iter_mut().find(|(held, _)| *held == space) {
                Some((_, held)) => held.push((routine, event)),
                None => by_space.push((space, vec![(routine, event)])),
            }
        }
        for (space, group) in by_space {
            let ask = EventAsk {
                space,
                since: UnixSeconds(since.0 - OVERLAP),
                kinds: group.iter().map(|(_, event)| event.kind.clone()).collect(),
            };
            for Seen { event, kind } in self.host.events(ask).await {
                if !self.remember(&event) {
                    continue;
                }
                for (routine, watch) in &group {
                    if watch.covers(&kind, &event.space) {
                        self.apply(RoutineEvent::Saw {
                            routine: *routine,
                            event: event.clone(),
                            kind: kind.clone(),
                            now,
                        })
                        .await;
                    }
                }
            }
        }
    }
}
