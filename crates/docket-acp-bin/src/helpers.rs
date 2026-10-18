//! The helpers an editor sees, read from companiond's hub over the bus. [`BusHelpers`] keeps what
//! it last read (the list, each helper's jobs, the sentences the board told) and refreshes it when
//! the daemon says something changed; [`LiveHelpers`] is it when the daemon answers and
//! [`NoHelpers`] when it does not, so an editor without helpers works as it did before them.

use companion_client::{Changes, DbusHelpers, HelpersTransport};
use companion_wire::{HelperLine, JobLine, SayReply, ToldWire};
use docket_acp::{ConversationTurns, HelperRefusal, HelperRow, Helpers, NoHelpers};
use docket_client::{DbusTransport, Intents, Transport as IntentsTransport, TransportError};
use docket_core::{ContextKeep, Keep, Origin, TurnEnd, TurnId, TurnIn, UserTurn};
use futures_util::StreamExt;
use prov::{HelperId, SessionId};
use std::collections::{BTreeMap, VecDeque};
use std::future::Future;

/// How long the editor waits for the daemon to answer before it goes on without helpers.
const ANSWER_WITHIN: std::time::Duration = std::time::Duration::from_secs(2);

/// What the person is told when an editor asks to start a job: it is given in their own words
/// where helpers are kept.
const NO_JOBS_FROM_HERE: &str =
    "A job is given to a helper from the Helpers window, in your own words.";

/// What the person is told when their words could not be taken to the helper.
const NOT_RECORDED: &str = "Your words could not be recorded, so nothing was sent.";
/// What the person is told when the daemon that keeps the helpers does not answer a turn.
const NOT_ANSWERED: &str = "The helper could not be reached.";

/// Where the person's turn in a helper's conversation is recorded. The router takes it from the
/// person's own surface only (the editor, here): a model cannot say what the person said, and the
/// daemon that runs the helper's turn is not such a surface.
pub trait Records: Send + Sync {
    /// Records `turn` in `session`; its number is what the helper's turn is run for.
    fn record(
        &self,
        session: &SessionId,
        turn: &UserTurn,
    ) -> impl Future<Output = Result<TurnId, HelperRefusal>> + Send;

    /// Says that the turn numbered `turn` is over, however it ended.
    fn ended(
        &self,
        session: &SessionId,
        turn: TurnId,
        end: TurnEnd,
    ) -> impl Future<Output = ()> + Send;
}

/// No router to record with: a helper cannot be spoken to.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoRecords;

impl Records for NoRecords {
    async fn record(
        &self,
        _session: &SessionId,
        _turn: &UserTurn,
    ) -> Result<TurnId, HelperRefusal> {
        Err(HelperRefusal(NOT_RECORDED.to_owned()))
    }

    async fn ended(&self, _session: &SessionId, _turn: TurnId, _end: TurnEnd) {}
}

/// The router over `T`, asked as the editor.
pub struct RouterRecords<T: IntentsTransport> {
    intents: Intents<T>,
}

impl<T: IntentsTransport> RouterRecords<T> {
    /// Records through `intents`.
    pub fn over(intents: Intents<T>) -> Self {
        Self { intents }
    }
}

impl<T: IntentsTransport> std::fmt::Debug for RouterRecords<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RouterRecords")
    }
}

/// The chips of context a turn in a conversation keeps: none.
fn kept_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

impl<T: IntentsTransport> Records for RouterRecords<T> {
    async fn record(&self, session: &SessionId, turn: &UserTurn) -> Result<TurnId, HelperRefusal> {
        let said = TurnIn {
            text: turn.text.clone(),
            origin: Origin::InWindowField,
            keep: kept_nothing(),
            via: turn.via,
        };
        self.intents
            .session_turn(session.clone(), said)
            .await
            .map_err(|_| HelperRefusal(NOT_RECORDED.to_owned()))
    }

    async fn ended(&self, session: &SessionId, turn: TurnId, end: TurnEnd) {
        // The turn is over either way; a router that cannot be told leaves nothing for the person
        // to do, and its own timeout ends the turn's window.
        let _ = self
            .intents
            .session_turn_ended(session.clone(), turn, end)
            .await;
    }
}

/// The helpers as companiond last told them.
pub struct BusHelpers<T: HelpersTransport, R = NoRecords> {
    transport: T,
    records: R,
    lines: Vec<HelperLine>,
    jobs: BTreeMap<HelperId, Vec<JobLine>>,
    told: VecDeque<ToldWire>,
    after: u64,
    changes: Changes,
}

impl<T: HelpersTransport, R> std::fmt::Debug for BusHelpers<T, R> {
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
    /// [`BusHelpers::connect_recording`] with no router: the helpers are shown, not spoken to.
    pub async fn connect(transport: T) -> Result<Self, TransportError> {
        Self::connect_recording(transport, NoRecords).await
    }
}

impl<T: HelpersTransport, R: Records> BusHelpers<T, R> {
    /// Subscribes to changes, then reads the helpers once; the person's words to a helper are
    /// recorded through `records`. An error means the daemon did not answer, and the editor goes
    /// on without helpers.
    pub async fn connect_recording(transport: T, records: R) -> Result<Self, TransportError> {
        let changes = transport.changes().await?;
        let mut helpers = Self {
            transport,
            records,
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

impl<T: HelpersTransport, R: Records> ConversationTurns for BusHelpers<T, R> {
    /// The turn is recorded with the router as the editor's, then taken to the daemon that keeps
    /// the helper, which runs it. The router hears the turn ended whichever way it went.
    async fn say(&mut self, session: &SessionId, turn: UserTurn) -> Result<String, HelperRefusal> {
        let id = self.records.record(session, &turn).await?;
        let (end, said) = match self.transport.say(session, id.0, &turn.text).await {
            Ok(SayReply::Said(words)) => (TurnEnd::Answered, Ok(words)),
            Ok(SayReply::Refused(why)) => (TurnEnd::Failed, Err(HelperRefusal(why))),
            Err(_) => (TurnEnd::Failed, Err(HelperRefusal(NOT_ANSWERED.to_owned()))),
        };
        self.records.ended(session, id, end).await;
        said
    }
}

impl<T: HelpersTransport, R: Records> Helpers for BusHelpers<T, R> {
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
    Bus(Box<BusHelpers<DbusHelpers, RouterRecords<DbusTransport>>>),
    /// It did not.
    None(NoHelpers),
}

impl LiveHelpers {
    /// The helpers over `transport` if the daemon answers it, else none; the person's words to a
    /// helper are recorded through `intents`.
    pub async fn over(transport: Option<(DbusHelpers, Intents<DbusTransport>)>) -> Self {
        let Some((transport, intents)) = transport else {
            return LiveHelpers::None(NoHelpers);
        };
        let records = RouterRecords::over(intents);
        match tokio::time::timeout(
            ANSWER_WITHIN,
            BusHelpers::connect_recording(transport, records),
        )
        .await
        {
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

impl ConversationTurns for LiveHelpers {
    async fn say(&mut self, session: &SessionId, turn: UserTurn) -> Result<String, HelperRefusal> {
        match self {
            LiveHelpers::Bus(helpers) => helpers.say(session, turn).await,
            LiveHelpers::None(none) => none.say(session, turn).await,
        }
    }
}
