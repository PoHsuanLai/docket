//! All the system seams behind `docket_router::Seams`.

use crate::builtin::HostedLink;
use crate::grants::FileGrants;
use crate::infer::{InferdModel, InferdWriter, ReaderClient};
use crate::memory::AlmanacMemory;
use crate::sheet::SheetConfirmer;
use crate::sink::QueuedSink;
use action_review::InferReviewer;
use docket_core::Millis;
use docket_router::{Clock, Seams};
use prov::UnixSeconds;

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
}
