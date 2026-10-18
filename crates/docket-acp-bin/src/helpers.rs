//! The helpers an editor sees, read from companiond's hub over the bus. [`BusHelpers`] keeps what
//! it last read (the list, each helper's jobs, the sentences the board told) and refreshes it when
//! the daemon says something changed; [`LiveHelpers`] is it when the daemon answers and
//! [`NoHelpers`] when it does not, so an editor without helpers works as it did before them.

use companion_client::{Changes, DbusHelpers, HelpersTransport};
use companion_wire::{HelperLine, JobLine, ToldWire};
use docket_acp::{HelperRefusal, HelperRow, Helpers, NoHelpers};
use docket_client::TransportError;
use futures_util::StreamExt;
use prov::{HelperId, SessionId};
use std::collections::{BTreeMap, VecDeque};

/// How long the editor waits for the daemon to answer before it goes on without helpers.
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// What the person is told when an editor asks to start a job: it is given in their own words
/// where helpers are kept.
const NO_JOBS_FROM_HERE: &str =
    "A job is given to a helper from the Helpers window, in your own words.";

/// The helpers as companiond last told them.
pub struct BusHelpers<T: HelpersTransport> {
    transport: T,
    lines: Vec<HelperLine>,
    jobs: BTreeMap<HelperId, Vec<JobLine>>,
    told: VecDeque<ToldWire>,
    after: u64,
    changes: Changes,
}

impl<T: HelpersTransport> std::fmt::Debug for BusHelpers<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BusHelpers")
    }
}

fn row_of(line: &HelperLine) -> Option<HelperRow> {
    let session: SessionId = line.conversation.clone()?;
    Some(HelperRow {
        id: line.id.clone(),
        name: line.name.clone(),
        space: line.space.clone(),
        session,
    })
}

impl<T: HelpersTransport> BusHelpers<T> {
    /// Subscribes to changes, then reads the helpers once. An error means the daemon did not
    /// answer, and the editor goes on without helpers.
    pub async fn connect(transport: T) -> Result<Self, TransportError> {
        let changes = transport.changes().await?;
        let mut helpers = Self {
            transport,
            lines: Vec::new(),
            jobs: BTreeMap::new(),
            told: VecDeque::new(),
            after: 0,
            changes,
        };
        helpers.refresh().await?;
        Ok(helpers)
    }

    /// Reads the list, the sentences told since the last read and each helper's jobs. What was
    /// read is kept only once all of it has been, so a read that is dropped half way loses nothing.
    async fn refresh(&mut self) -> Result<(), TransportError> {
        let view = self.transport.list(self.after).await?;
        let mut jobs = BTreeMap::new();
        for line in &view.helpers {
            let card = self.transport.card(&line.id).await?;
            jobs.insert(line.id.clone(), card.jobs);
        }
        self.after = view.told.iter().map(|t| t.n).fold(self.after, u64::max);
        self.told.extend(view.told);
        self.lines = view.helpers;
        self.jobs = jobs;
        Ok(())
    }
}

impl<T: HelpersTransport> Helpers for BusHelpers<T> {
    fn rows(&self) -> Vec<HelperRow> {
        self.lines.iter().filter_map(row_of).collect()
    }

    fn jobs(&self, helper: &HelperId) -> Vec<JobLine> {
        self.jobs.get(helper).cloned().unwrap_or_default()
    }

    async fn open_job(&mut self, _helper: &HelperId) -> Result<SessionId, HelperRefusal> {
        Err(HelperRefusal(NO_JOBS_FROM_HERE.to_owned()))
    }

    async fn next_told(&mut self, watching: &[HelperId]) -> ToldWire {
        loop {
            let heard = self
                .told
                .iter()
                .position(|t| watching.contains(&t.helper))
                .and_then(|at| self.told.remove(at));
            if let Some(told) = heard {
                return told;
            }
            // Wait for the daemon to say something changed; a daemon that went away says nothing
            // more, and neither does this.
            if self.changes.next().await.is_none() {
                return std::future::pending().await;
            }
            if self.refresh().await.is_err() {
                return std::future::pending().await;
            }
        }
    }
}

/// The helpers of a running editor connection: the daemon's, or none.
#[derive(Debug)]
pub enum LiveHelpers {
    /// The daemon answered.
    Bus(Box<BusHelpers<DbusHelpers>>),
    /// It did not.
    None(NoHelpers),
}

impl LiveHelpers {
    /// The helpers over `transport` if the daemon answers it, else none.
    pub async fn over(transport: Option<DbusHelpers>) -> Self {
        let Some(transport) = transport else {
            return LiveHelpers::None(NoHelpers);
        };
        match tokio::time::timeout(ANSWER_WITHIN, BusHelpers::connect(transport)).await {
            Ok(Ok(helpers)) => LiveHelpers::Bus(Box::new(helpers)),
            // The daemon is not there, is slow to start, or does not speak to this process.
            Ok(Err(_)) | Err(_) => LiveHelpers::None(NoHelpers),
        }
    }
}

impl Helpers for LiveHelpers {
    fn rows(&self) -> Vec<HelperRow> {
        match self {
            LiveHelpers::Bus(helpers) => helpers.rows(),
            LiveHelpers::None(none) => none.rows(),
        }
    }

    fn jobs(&self, helper: &HelperId) -> Vec<JobLine> {
        match self {
            LiveHelpers::Bus(helpers) => helpers.jobs(helper),
            LiveHelpers::None(none) => none.jobs(helper),
        }
    }

    async fn open_job(&mut self, helper: &HelperId) -> Result<SessionId, HelperRefusal> {
        match self {
            LiveHelpers::Bus(helpers) => helpers.open_job(helper).await,
            LiveHelpers::None(none) => none.open_job(helper).await,
        }
    }

    async fn next_told(&mut self, watching: &[HelperId]) -> ToldWire {
        match self {
            LiveHelpers::Bus(helpers) => helpers.next_told(watching).await,
            LiveHelpers::None(none) => none.next_told(watching).await,
        }
    }
}
