//! For tests only: a `SessionHost` over `FakeBackend`s and a shared `MemoryLog`. It writes the
//! log the way a real host does (opening, turn, call, step, close, each before it answers) and
//! hands each session the next scripted backend. Fork is not scripted.

use crate::backend::{
    BackendEvent, CallEvent, HostFault, SessionBackend, SessionHost, SheetChoice, StartSession,
};
use crate::entry::{EndCause, Opening, Seq, SessionEntry};
use crate::export::{SessionExport, export};
use crate::fake::{FakeBackend, MemoryLog};
use crate::log::{SessionLog, read_all};
use crate::plan::ResumePlan;
use crate::resume::resume_plan;
use docket_core::{ConfirmId, UserTurn};
use prov::SessionId;
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::Arc;

/// A host whose sessions are scripted: one list of per-turn event lists for each session it opens
/// or resumes, in that order.
#[derive(Debug)]
pub struct FakeHost {
    log: Arc<MemoryLog>,
    scripts: VecDeque<Vec<Vec<BackendEvent>>>,
    backends: BTreeMap<SessionId, FakeBackend>,
    next_seq: BTreeMap<SessionId, Seq>,
    closed: BTreeSet<SessionId>,
    minted: u32,
    /// The sessions it was asked to cancel, in order.
    pub cancelled: Vec<SessionId>,
    /// The choices it was given on sheets, in order.
    pub sheets: Vec<(ConfirmId, SheetChoice)>,
    /// The turns it was given, by session, in order.
    pub turns: Vec<(SessionId, UserTurn)>,
}

impl FakeHost {
    /// A host writing to `log`; each session it opens takes the next script.
    pub fn new(log: Arc<MemoryLog>, scripts: Vec<Vec<Vec<BackendEvent>>>) -> Self {
        Self {
            log,
            scripts: scripts.into(),
            backends: BTreeMap::new(),
            next_seq: BTreeMap::new(),
            closed: BTreeSet::new(),
            minted: 0,
            cancelled: Vec::new(),
            sheets: Vec::new(),
            turns: Vec::new(),
        }
    }

    /// The same host numbering the sessions it opens from `n`, so two hosts over one log do not
    /// mint the same id.
    pub fn numbered_from(mut self, n: u32) -> Self {
        self.minted = n.saturating_sub(1);
        self
    }

    async fn append(&mut self, session: &SessionId, entry: SessionEntry) -> Result<(), HostFault> {
        let seq = match self.next_seq.get(session) {
            Some(seq) => *seq,
            None => Seq(read_all(&*self.log, session).await?.len() as u64),
        };
        self.log.append(session, seq, &entry).await?;
        self.next_seq.insert(session.clone(), seq.next());
        Ok(())
    }

    fn backend(&mut self, session: &SessionId) -> Result<&mut FakeBackend, HostFault> {
        self.backends
            .get_mut(session)
            .ok_or(HostFault::NoSuchSession)
    }
}

impl SessionHost for FakeHost {
    async fn open(&mut self, opening: Opening) -> Result<SessionId, HostFault> {
        self.minted += 1;
        let session = SessionId::parse(&format!("s-{}", self.minted))
            .map_err(|_| HostFault::NoSuchSession)?;
        self.append(&session, SessionEntry::Opened(opening.clone()))
            .await?;
        let mut backend = FakeBackend::new(self.scripts.pop_front().unwrap_or_default());
        backend
            .start(StartSession {
                session: session.clone(),
                opening,
            })
            .await?;
        self.backends.insert(session.clone(), backend);
        Ok(session)
    }

    async fn resume(&mut self, session: &SessionId) -> Result<ResumePlan, HostFault> {
        let rows = read_all(&*self.log, session).await?;
        let plan = resume_plan(&rows)?;
        if !self.backends.contains_key(session) {
            let script = self.scripts.pop_front().unwrap_or_default();
            self.backends
                .insert(session.clone(), FakeBackend::new(script));
        }
        self.backend(session)?.resume(&plan).await?;
        Ok(plan)
    }

    async fn turn(&mut self, session: &SessionId, turn: UserTurn) -> Result<(), HostFault> {
        if self.closed.contains(session) {
            return Err(HostFault::NotOpen);
        }
        self.backend(session)?;
        self.append(session, SessionEntry::Turn(turn.clone()))
            .await?;
        self.turns.push((session.clone(), turn.clone()));
        self.backend(session)?.turn(turn).await?;
        Ok(())
    }

    async fn next_event(&mut self, session: &SessionId) -> Result<Option<BackendEvent>, HostFault> {
        let event = self.backend(session)?.next_event().await;
        match &event {
            Some(BackendEvent::Call(CallEvent::Started(open))) => {
                self.append(session, SessionEntry::Call(open.clone()))
                    .await?;
            }
            Some(BackendEvent::Call(CallEvent::Ended(step))) => {
                self.append(session, SessionEntry::Step(step.clone()))
                    .await?;
            }
            _ => {}
        }
        Ok(event)
    }

    async fn answer_sheet(
        &mut self,
        session: &SessionId,
        id: &ConfirmId,
        choice: SheetChoice,
    ) -> Result<(), HostFault> {
        self.backend(session)?;
        self.sheets.push((id.clone(), choice));
        Ok(())
    }

    async fn cancel(&mut self, session: &SessionId) -> Result<(), HostFault> {
        self.backend(session)?.cancel().await;
        self.cancelled.push(session.clone());
        Ok(())
    }

    async fn close(&mut self, session: &SessionId, cause: EndCause) -> Result<(), HostFault> {
        self.append(session, SessionEntry::Closed(cause)).await?;
        self.backend(session)?.close().await;
        self.closed.insert(session.clone());
        Ok(())
    }

    async fn fork(&mut self, _session: &SessionId, _at: Seq) -> Result<SessionId, HostFault> {
        Err(HostFault::NoSuchSession)
    }

    async fn export(&self, session: &SessionId) -> Result<SessionExport, HostFault> {
        let rows = read_all(&*self.log, session).await?;
        Ok(export(session, &rows)?)
    }
}
