//! For tests only: a scripted `SessionBackend` and an in-memory `SessionLog`.

use crate::backend::{BackendEvent, BackendFault, Resumed, SessionBackend, StartSession, TurnEnd};
use crate::codec::{Logged, decode, encode};
use crate::entry::{BackendKind, Seq, SessionEntry};
use crate::log::{Appended, LogFault, LogPage, PageSize, SessionLog};
use crate::plan::ResumePlan;
use docket_core::UserTurn;
use prov::SessionId;
use std::collections::{BTreeMap, VecDeque};
use std::sync::Mutex;

/// What the fake backend is doing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Running {
    Stopped,
    Idle,
    Answering,
}

/// A backend that answers each turn with the next scripted list of events (and `TurnEnd::Done`
/// when a script does not end in one).
#[derive(Debug)]
pub struct FakeBackend {
    running: Running,
    script: VecDeque<Vec<BackendEvent>>,
    pending: VecDeque<BackendEvent>,
    /// The turns it was given, in order.
    pub turns: Vec<UserTurn>,
}

impl FakeBackend {
    /// A backend with one script per turn it will be given.
    pub fn new(script: Vec<Vec<BackendEvent>>) -> Self {
        Self {
            running: Running::Stopped,
            script: script.into(),
            pending: VecDeque::new(),
            turns: Vec::new(),
        }
    }
}

impl SessionBackend for FakeBackend {
    fn kind(&self) -> BackendKind {
        BackendKind::Fake
    }

    async fn start(&mut self, _open: StartSession) -> Result<(), BackendFault> {
        self.running = Running::Idle;
        Ok(())
    }

    async fn resume(&mut self, _plan: &ResumePlan) -> Result<Resumed, BackendFault> {
        self.running = Running::Idle;
        Ok(Resumed::Restored)
    }

    async fn turn(&mut self, turn: UserTurn) -> Result<(), BackendFault> {
        match self.running {
            Running::Stopped => Err(BackendFault::NotRunning),
            Running::Answering => Err(BackendFault::Busy),
            Running::Idle => {
                self.turns.push(turn);
                let mut events = self.script.pop_front().unwrap_or_default();
                if !matches!(events.last(), Some(BackendEvent::TurnEnd(_))) {
                    events.push(BackendEvent::TurnEnd(TurnEnd::Done));
                }
                self.pending = events.into();
                self.running = Running::Answering;
                Ok(())
            }
        }
    }

    async fn next_event(&mut self) -> Option<BackendEvent> {
        let event = self.pending.pop_front()?;
        if matches!(event, BackendEvent::TurnEnd(_)) {
            self.running = Running::Idle;
        }
        Some(event)
    }

    async fn cancel(&mut self) {
        self.pending = VecDeque::from([BackendEvent::TurnEnd(TurnEnd::Cancelled)]);
    }

    async fn close(&mut self) {
        self.running = Running::Stopped;
        self.pending.clear();
    }
}

/// How the in-memory log answers its next append.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogMood {
    /// It stores.
    Storing,
    /// It refuses every append.
    Refusing,
    /// It does not answer.
    Down,
    /// It stores everything but taint entries, which it refuses: the fault a write-ahead taint
    /// has to survive.
    RefusingTaint,
}

/// An in-memory `SessionLog` that stores the encoded bodies, so a test reads back exactly what a
/// store would hold.
#[derive(Debug)]
pub struct MemoryLog {
    bodies: Mutex<BTreeMap<SessionId, Vec<(String, String)>>>,
    mood: Mutex<LogMood>,
    /// How many more appends it stores before it stops answering (a crash), if limited.
    allowance: Mutex<Option<u32>>,
}

impl MemoryLog {
    /// An empty log that stores.
    pub fn new() -> Self {
        Self {
            bodies: Mutex::new(BTreeMap::new()),
            mood: Mutex::new(LogMood::Storing),
            allowance: Mutex::new(None),
        }
    }

    /// Stores `appends` more entries and then answers nothing, as a store does when the process
    /// is killed between two appends. `set_mood(Storing)` and `no_crash` bring it back.
    pub fn crash_after(&self, appends: u32) {
        if let Ok(mut a) = self.allowance.lock() {
            *a = Some(appends);
        }
    }

    /// Ends a `crash_after`: the store answers again, holding what it stored before.
    pub fn no_crash(&self) {
        if let Ok(mut a) = self.allowance.lock() {
            *a = None;
        }
    }

    /// Forgets every session (a harness that starts its counters again must not meet old ones).
    pub fn clear(&self) {
        if let Ok(mut all) = self.bodies.lock() {
            all.clear();
        }
    }

    /// How many entries it holds in all.
    pub fn stored(&self) -> usize {
        self.bodies
            .lock()
            .map_or(0, |all| all.values().map(Vec::len).sum())
    }

    /// Changes how it answers appends from now on.
    pub fn set_mood(&self, mood: LogMood) {
        if let Ok(mut m) = self.mood.lock() {
            *m = mood;
        }
    }

    /// Puts a raw stored body at the end of a session, as an older writer or a newer one left it.
    pub fn put_raw(&self, session: &SessionId, kind: &str, json: &str) {
        if let Ok(mut all) = self.bodies.lock() {
            all.entry(session.clone())
                .or_default()
                .push((kind.to_owned(), json.to_owned()));
        }
    }
}

impl Default for MemoryLog {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionLog for MemoryLog {
    async fn append(
        &self,
        session: &SessionId,
        seq: Seq,
        entry: &SessionEntry,
    ) -> Result<Appended, LogFault> {
        match self.mood.lock().map(|m| *m) {
            Ok(LogMood::Storing) => {}
            Ok(LogMood::RefusingTaint) if !matches!(entry, SessionEntry::Taint(_)) => {}
            Ok(LogMood::Refusing | LogMood::RefusingTaint) => return Err(LogFault::Refused),
            Ok(LogMood::Down) | Err(_) => return Err(LogFault::Unavailable),
        }
        if let Ok(mut allowance) = self.allowance.lock()
            && let Some(left) = allowance.as_mut()
        {
            if *left == 0 {
                return Err(LogFault::Unavailable);
            }
            *left -= 1;
        }
        let encoded = encode(seq, entry).map_err(|_| LogFault::Encode)?;
        let mut all = self.bodies.lock().map_err(|_| LogFault::Unavailable)?;
        let rows = all.entry(session.clone()).or_default();
        let expected = Seq(rows.len() as u64);
        if seq != expected {
            return Err(LogFault::OutOfOrder { expected });
        }
        rows.push((encoded.kind, encoded.json));
        Ok(Appended { seq })
    }

    async fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> Result<LogPage, LogFault> {
        let all = self.bodies.lock().map_err(|_| LogFault::Unavailable)?;
        let rows = all.get(session).map(Vec::as_slice).unwrap_or_default();
        let start = from.map_or(0, |s| s.0 as usize).min(rows.len());
        let take = size.0.0 as usize;
        let end = start.saturating_add(take).min(rows.len());
        let logged: Vec<Logged> = rows[start..end]
            .iter()
            .enumerate()
            .map(|(i, (kind, json))| {
                let slug = kind.rsplit('.').next().unwrap_or_default();
                decode(slug, json, Seq((start + i) as u64))
            })
            .collect();
        let next = (end < rows.len()).then_some(Seq(end as u64));
        Ok(LogPage { rows: logged, next })
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        let all = self.bodies.lock().map_err(|_| LogFault::Unavailable)?;
        Ok(all
            .iter()
            .filter(|(_, rows)| {
                rows.first()
                    .is_some_and(|(kind, _)| kind.ends_with(".opened"))
            })
            .map(|(id, _)| id.clone())
            .collect())
    }
}
