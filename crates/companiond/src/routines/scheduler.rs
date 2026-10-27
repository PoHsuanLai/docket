//! The scheduler: the routine book's owner. A small timer inside companiond (not a daemon of its
//! own) ticks every few seconds; each tick asks which runs ended, which times are due, and what
//! the almanac saw. A time that came while the computer slept or the assistant was down shows as a
//! late tick, and the book skips it (or runs it once, when the person said so). Systemd user
//! timers, written from the same schedules, only start companiond at the time.

use super::host::Host;
use companion_core::{EventWatch, Settle, ended_events, event_asks, group_by_space};
use docket_core::Clock;
use docket_core::routine::{
    RoutineBook, RoutineEffect, RoutineEvent, RoutineId, RoutineRecord, step,
};
use prov::{SpaceId, UnixSeconds};
use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How often the timer ticks.
const POLL: Duration = Duration::from_secs(5);

struct State {
    book: RoutineBook,
    /// What the host was last told: the timers installed, the need to stay awake.
    settle: Settle,
    /// When the almanac was last asked, and the events already acted on.
    watch: EventWatch,
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
        let watch = EventWatch::new(clock.now());
        Self {
            host,
            clock,
            state: Mutex::new(State {
                book: RoutineBook::default(),
                settle: Settle::default(),
                watch,
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
            state.watch.restart(now);
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
        let groups = group_by_space(
            records
                .into_iter()
                .filter_map(|record| Some((self.host.space_of(record.helper())?, record))),
        );
        for (space, records) in groups {
            self.host.save(&space, records).await;
        }
    }

    /// Tells the host what changed since it was last told: the timers wanted, and whether the
    /// desktop must stay awake (only while a run is in progress).
    fn settle(&self) {
        let settled = self.with(|state| state.settle.settle(&state.book));
        if let Some(units) = settled.units {
            self.host.install(&units);
        }
        if let Some(need) = settled.awake {
            self.host.awake(need);
        }
    }

    /// One tick: the runs that ended, the times that are due, and the events the almanac saw.
    pub async fn tick(&self) {
        let now = self.clock.now();
        let ended = self.with(|state| ended_events(&state.book, now, |job| self.host.job_end(job)));
        for event in ended {
            self.apply(event).await;
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

    async fn poll_events(&self, now: UnixSeconds) {
        let asks = self.with(|state| {
            let since = state.watch.due(now)?;
            Some(event_asks(&state.book, since, |helper| {
                self.host.space_of(helper)
            }))
        });
        for watch in asks.unwrap_or_default() {
            for seen in self.host.events(watch.ask.clone()).await {
                if !self.with(|state| state.watch.remember(&seen.event)) {
                    continue;
                }
                for routine in watch.covering(&seen) {
                    self.apply(RoutineEvent::Saw {
                        routine,
                        event: seen.event.clone(),
                        kind: seen.kind.clone(),
                        now,
                    })
                    .await;
                }
            }
        }
    }
}
