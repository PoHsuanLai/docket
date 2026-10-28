//! The bodies of `org.quire.Intents1` that are not the request or reply enums themselves: the
//! envelope, the asks and answers of the session and gate members, recall, notes, stored
//! sessions and the wire refusal.

use almanac_core::{
    Episode, EpisodeId, EventSummary, InjectQuery, JsonText, Narrative, RecentQuery,
};
use docket_core::{
    CallRefusal, ConfirmEnd, EpisodeLine, PrimerText, ProfileLine, ReadFault, ReaderAsk,
    RecalledLine, Reveal, SendRefusal, UndoFault,
};
use docket_permissions_core::TaskPolicy;
use docket_vocab::{ActionRef, IntentsVocab, NoteSlug, TurnId};
use porter_core::{AppName, Count};
use prov::{RunId, SessionId, SpaceId};
use serde::{Deserialize, Serialize};

/// A body with the vocabulary it is written in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Envelope<T> {
    /// The vocabulary.
    pub vocab: IntentsVocab,
    /// The body.
    pub body: T,
}

/// The planner asks the reader (`Session.Read`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadAsk {
    /// The ask.
    pub ask: ReaderAsk,
}

/// A policy change that needs the person (`Session.Widen`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WidenAsk {
    /// The turn whose words the person confirms against.
    pub turn: TurnId,
    /// The wider policy.
    pub change: TaskPolicy,
}

/// How a widening ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WidenAnswer {
    /// The person confirmed: the wider policy is active.
    Applied,
    /// The old policy stays.
    Refused(ConfirmEnd),
}

/// What the router reads from memory for a companion session (`Session.Recall`): the router
/// applies the labels, so untrusted text comes back as handles and the planner never reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RecallAsk {
    /// Recent activity in the session's Space.
    Recent(RecentQuery),
    /// Automatic top-k recall for this turn.
    Inject(InjectQuery),
    /// The Space's recent episodes as the planner reads them (the query's kinds and bodies are
    /// the router's: `companion.episode`, with bodies): the skeleton as trusted lines, a
    /// narrative by handle.
    Episodes(RecentQuery),
    /// The Space's primer: at most 200 lines of what is kept.
    Primer,
    /// The facts the person stated themselves at the desktop scope.
    Profile,
    /// The Spaces a restart should read: every Space memory keeps records for, and the ones the
    /// router itself holds a session or a task in. Asked in any session (the Space it is in does
    /// not matter).
    Spaces,
}

/// One recent event as the planner may read it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentLine {
    /// What and when.
    pub summary: EventSummary,
    /// Its effect.
    pub effect: prov::Effect,
    /// Its provenance.
    pub label: prov::Label,
    /// Its text: plain when trusted, a handle when not.
    pub text: Option<Reveal<String>>,
    /// The body's serde JSON, when the query asked for bodies and the label is trusted: an
    /// untrusted body never reaches the companion (its text is the handle above).
    pub body: Option<JsonText>,
}

/// What a recall returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RecallView {
    /// Recent events.
    Recent(Vec<RecentLine>),
    /// Recalled items.
    Hits(Vec<RecalledLine>),
    /// Recent episodes, skeleton as trusted lines.
    Episodes(Vec<EpisodeLine>),
    /// The Space's primer.
    Primer(PrimerText),
    /// The person's own pinned facts.
    Profile(Vec<ProfileLine>),
    /// The Spaces to read on a restart, each once.
    Spaces(Vec<prov::SpaceId>),
}

/// A record companiond keeps about its own sessions (`companion-wire`'s `SessionRecord`, in its
/// serde form): the router stores it for the Space the session is in, as
/// `Area { Companion }` of kind `companion.session.<slug>`, trusted and private to the Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNote {
    /// What kind of record: the tail of its kind tag.
    pub slug: NoteSlug,
    /// The record's serde JSON.
    pub json: JsonText,
}

/// A record of the helpers (a card, a job or a post) the companion hands the router to keep, in
/// its serde form: the router stores it for the Space the session is in, as `Area { Helpers }`
/// of the kind's tag (`helpers.card`, `helpers.job`, `helpers.post`), trusted and private to the
/// Space.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelperNote {
    /// Which kind of record.
    pub kind: companion_model::helper::HelperRecordKind,
    /// The record's serde JSON.
    pub json: JsonText,
}

/// What the companion hands the router to record (`Session.Note`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum NoteAsk {
    /// An episode as it is: a side conversation's, or a worker's skeleton when its final report
    /// ended the task before the router could write one.
    Episode(Box<Episode>),
    /// The idle pass's narrative of an episode the router already holds the skeleton of: the
    /// router merges it into its own skeleton (the companion never restates the skeleton) and
    /// records the narrated successor.
    Narrative {
        /// Which episode (the task's id).
        episode: EpisodeId,
        /// What the model wrote.
        narrative: Narrative,
    },
    /// The task of this session is over: the router ends it and leaves its skeleton now, as
    /// closing the session would, but the session stays open (its handles can still be shown)
    /// until `Session.Close`. A task already ended writes nothing.
    End,
    /// A record of the companion's sessions.
    Record(SessionNote),
    /// A record of the helpers: a card, a job or a post.
    Helper(HelperNote),
    /// The host's desk refused a helper action the router had allowed and recorded as done (the
    /// router answers `companion.helper.post_job` and its two kin before the desk acts). The
    /// router amends its own record of the session's latest such call to end in `why`, so its
    /// ledger and the episode made from it never say done for what was not.
    DeskRefused {
        /// The helper action.
        action: ActionRef,
        /// What the desk said, as the call's refusal.
        why: CallRefusal,
    },
}

/// What to list of the undo journal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct JournalFilter {
    /// Only this run.
    pub run: Option<RunId>,
    /// Only this session.
    pub session: Option<SessionId>,
    /// At most this many, newest first.
    pub limit: Count,
}

/// The CUA consent grant (`Gate.Grant`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantAsk {
    /// The app.
    pub app: AppName,
    /// The Space.
    pub space: SpaceId,
    /// What the caller's own setting for this app says: allow always (the default, and what a
    /// request from before this field meant), ask every run, or never. The router keeps its own
    /// store the last word: an entry it holds can only make this stricter.
    #[serde(default)]
    pub level: docket_permissions_core::Level,
    /// The screen run this request is for. With it, the person's yes covers that run alone; a
    /// request from before this field names no run, and covers every live screen run in the
    /// Space. `cuad` should send it, so two runs in one Space each get their own start sheet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<prov::RunId>,
}

/// The switch of one conversation's mode (`Session.SetMode`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionModeAsk {
    /// The conversation.
    pub session: SessionId,
    /// The mode to switch to.
    pub mode: docket_permissions_core::Mode,
}

/// How a grant request ended.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum GrantAnswer {
    /// The person allowed it.
    Granted,
    /// They did not.
    Refused(ConfirmEnd),
    /// Refused without asking: the person's "never" (or the administrator's) covers it.
    Never,
}

/// How far an undo got.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UndoReport {
    /// How many entries were undone.
    pub undone: Count,
    /// Why it stopped early, if it did.
    pub stopped: Option<UndoFault>,
}

/// Why a request got no answer of its own kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum WireRefusal {
    /// This caller's role may not make this request.
    NotAllowed,
    /// The session is unknown or closed.
    NoSuchSession,
    /// The request is malformed.
    Malformed,
    /// A call was refused.
    Call(CallRefusal),
    /// A message was not delivered.
    Send(SendRefusal),
    /// A read gave no answer.
    Read(ReadFault),
    /// The administrator holds every conversation in Manual, so the mode cannot be switched.
    Locked,
}

/// A handle's content as the reader receives it: the text and the label the router holds for it,
/// so the reader classes what it sends to a model by the handle's own class (mail, not "the
/// person's words").
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Resolved {
    /// The text.
    pub text: String,
    /// Its label in the session's table.
    pub label: prov::Label,
}

/// A handle's content as the screen receives it: the text, and the label the router holds for it
/// so the screen can mark content that is not the person's own. Never for a model.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Displayed {
    /// The text.
    pub text: String,
    /// Its label in the session's table.
    pub label: prov::Label,
}

/// What `.Session.Stored` is asked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StoredAsk {
    /// The sessions the caller may bring back, oldest first.
    List,
    /// One session's rows from a position, at most `size`.
    Rows {
        /// The session.
        session: SessionId,
        /// The position to start from (the first row when none).
        from: Option<u64>,
        /// How many rows at most.
        size: u32,
    },
    /// A new session that keeps `session`'s log up to and including the row `at` (the router
    /// writes the child's log; the caller may bring the parent back, as for `Rows`).
    Fork {
        /// The session to fork.
        session: SessionId,
        /// The last row the fork keeps.
        at: u64,
    },
}

/// One row of a stored session: its position and its body, which `docket-session` writes and reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRow {
    /// The position.
    pub seq: u64,
    /// The row as JSON.
    pub json: String,
}

/// What `.Session.Stored` answers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StoredView {
    /// The sessions.
    Sessions(Vec<SessionId>),
    /// A page of rows.
    Rows {
        /// The rows, oldest first.
        rows: Vec<StoredRow>,
        /// Where the next page starts; none at the end.
        next: Option<u64>,
    },
    /// The session a fork made.
    Forked(SessionId),
}
