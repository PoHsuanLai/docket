//! All the system seams behind `docket_router::Seams`.

use crate::builtin::HostedLink;
use crate::grants::FileGrants;
use crate::infer::{InferdModel, InferdWriter, ReaderClient};
use crate::sheet::SheetConfirmer;
use action_review::InferReviewer;
use docket_checkpoint_git::{GitStore, StdGitRun};
use docket_core::Millis;
use docket_memory::QueuedSink;
use docket_memory::{AlmanacMemory, AlmanacSessionLog};
use docket_router::{Clock, NoLog, Seams};
use docket_session::{Appended, LogFault, LogPage, PageSize, Seq, SessionEntry, SessionLog};
use prov::{SessionId, UnixSeconds};

/// The system clock: the one place intentd reads the time.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> UnixSeconds {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        UnixSeconds(i64::try_from(seconds).unwrap_or(i64::MAX))
    }

    async fn after(&self, wait: Millis) {
        tokio::time::sleep(std::time::Duration::from_millis(u64::from(wait.0))).await;
    }
}

/// The sessions' log of the running daemon: memoryd's, or none. The daemon reads the log once at
/// start (`Router::adopt_sessions`); a memoryd that cannot serve it (not running, or too old to
/// have `Entries` and `RecordDurable` on the bus) leaves the sessions unrecorded for this run,
/// and the daemon says so, rather than refusing every call for want of a record.
#[derive(Debug)]
pub enum DaemonLog<M: almanac_client::Transport> {
    /// Sessions are recorded in memoryd.
    Almanac(AlmanacSessionLog<M, SystemClock>),
    /// Sessions are not recorded.
    Off(NoLog),
}

impl<M: almanac_client::Transport> SessionLog for DaemonLog<M> {
    async fn append(
        &self,
        session: &SessionId,
        seq: Seq,
        entry: &SessionEntry,
    ) -> Result<Appended, LogFault> {
        match self {
            DaemonLog::Almanac(log) => log.append(session, seq, entry).await,
            DaemonLog::Off(log) => log.append(session, seq, entry).await,
        }
    }

    async fn page(
        &self,
        session: &SessionId,
        from: Option<Seq>,
        size: PageSize,
    ) -> Result<LogPage, LogFault> {
        match self {
            DaemonLog::Almanac(log) => log.page(session, from, size).await,
            DaemonLog::Off(log) => log.page(session, from, size).await,
        }
    }

    async fn sessions(&self) -> Result<Vec<SessionId>, LogFault> {
        match self {
            DaemonLog::Almanac(log) => log.sessions().await,
            DaemonLog::Off(log) => log.sessions().await,
        }
    }
}

/// The seams of the running daemon: apps and the sheet over the session bus, the built-in
/// providers in process, models over
/// inferd (`P` is porter-client's transport), memory over memoryd (`M` is almanac-client's).
#[derive(Debug)]
pub struct SystemSeams<P: porter_client::Transport, M: almanac_client::Transport> {
    /// The apps, and the two built-in providers answered in process.
    pub link: HostedLink<M>,
    /// The sheet.
    pub confirmer: SheetConfirmer,
    /// The reviewer cascade.
    pub reviewer: InferReviewer<InferdModel<P>>,
    /// The consent store.
    pub grants: FileGrants,
    /// The event log's queue.
    pub sink: QueuedSink,
    /// The clock.
    pub clock: SystemClock,
    /// Memory.
    pub memory: AlmanacMemory<M>,
    /// The policy writer.
    pub writer: InferdWriter<P>,
    /// The reader.
    pub reader: ReaderClient,
    /// The sessions' durable log, in memoryd.
    pub log: DaemonLog<M>,
    /// The restore points of the workspaces sessions work in, kept in their git repositories.
    pub checkpoints: GitStore<StdGitRun>,
}

impl<P: porter_client::Transport, M: almanac_client::Transport> Seams for SystemSeams<P, M> {
    type Link = HostedLink<M>;
    type Confirm = SheetConfirmer;
    type Review = InferReviewer<InferdModel<P>>;
    type Grants = FileGrants;
    type Sink = QueuedSink;
    type Time = SystemClock;
    type Memory = AlmanacMemory<M>;
    type Writer = InferdWriter<P>;
    type Reading = ReaderClient;
    type Log = DaemonLog<M>;
    type Checkpoints = GitStore<StdGitRun>;

    fn link(&self) -> &HostedLink<M> {
        &self.link
    }
    fn confirmer(&self) -> &SheetConfirmer {
        &self.confirmer
    }
    fn reviewer(&self) -> &Self::Review {
        &self.reviewer
    }
    fn grants(&self) -> &FileGrants {
        &self.grants
    }
    fn sink(&self) -> &QueuedSink {
        &self.sink
    }
    fn clock(&self) -> &SystemClock {
        &self.clock
    }
    fn memory(&self) -> &Self::Memory {
        &self.memory
    }
    fn writer(&self) -> &Self::Writer {
        &self.writer
    }
    fn reader(&self) -> &ReaderClient {
        &self.reader
    }
    fn log(&self) -> &Self::Log {
        &self.log
    }
    fn checkpoints(&self) -> &GitStore<StdGitRun> {
        &self.checkpoints
    }
}
