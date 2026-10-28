//! docket's vocabulary above `docket-vocab`, and the pure behaviour that is cheap to build with it:
//! the manifest and its validation, context and previews, calls and every way they end, the undo
//! journal, confirmation, budgets and the kill switch, the per-task policy, the planner and
//! reader contract, the roster, messages, tasks and their episodes, audit records, the proposed
//! configuration and the Intents1 wire. Ids, units, values, call requests, dates and the `Clock`
//! seam live in `docket-vocab`.
//!
//! Pure and portable: serde only. Names `prov` for who and what, `almanac-core` for the pure
//! shapes of memory (episodes, recall, recent), `cua-action` for the computer-use gate and
//! `model-provider` for the structured-output shapes `ValueSchema` renders through.

mod agent_app;
mod audit;
mod brief;
mod budget;
mod call;
mod caller;
mod checkpoint;
mod compact;
mod config;
mod confirm;
mod context;
mod derived;
mod exec_derive;
mod exec_facts;
mod exec_reach;
mod execute;
mod external;
mod floor;
mod gate;
mod grant;
mod grant_space;
mod hand;
mod handover;
pub mod helper;
mod index;
mod limit;
mod memory_link;
mod message;
mod mode;
mod pattern;
mod planner;
mod preview;
mod reach_menu;
mod reader;
mod review;
mod roster;
pub mod routine;
mod rules;
mod run_end;
mod schema;
mod skill;
mod space_access;
mod spend;
mod standing;
mod standing_match;
mod standing_offer;
mod started;
mod summon;
mod task;
mod task_policy;
mod trace;
mod undo;
mod wire;

pub use agent_app::{
    ACP_AGENT_APP, FILES_READ, FILES_SENSITIVE, FILES_WRITE, PermissionKind, REPORTED,
    TERMINAL_RUN, approves, covers_approval, is_agent_action,
};
pub use agent_app::{action as acp_agent_action, app as acp_agent_app};
pub use audit::{AuditRecord, ConfirmAnswerKind, DecidedBy, PolicyChangeKind};
pub use brief::{
    BRIEF_MAX, BriefEdit, BriefFault, BriefFlag, BriefSheet, BriefText, BriefWhy,
    FROM_CHECKED_BRIEF, flag_brief, from_checked_brief,
};
pub use budget::{
    Budget, BudgetKind, Cost, Halt, HaltCause, KillSwitch, Ledger, Reviewed, charge, halted,
};
pub use call::{
    ActivatedInvocation, ActivationToken, AppRefusal, ArgFault, CallEnd, CallProgress, CallRefusal,
    FailText, Follow, Invocation, Outcome, Undoable,
};
pub use caller::{CallerId, CallerRole, Member, is_terminal_scope};
pub use checkpoint::{
    CheckpointEvent, CheckpointFault, CheckpointId, CheckpointList, CheckpointNote, CheckpointRow,
    PlanDigest, RestorePlan, Retention, Rewind, SavedState, SkipReason, TurnEnd, TurnState,
    WorkPath, WorkPathError,
};
pub use compact::{Covers, SUMMARY_MAX_CHARS, Summary, SummaryFault, SummaryText};
pub use config::{
    AgentConfig, AssemblerBudget, BreakerLimits, IdleRules, ReviewTimeouts, SETTING_ROWS,
    STANDARD_BUDGET, SettingRow, SettingValue, TASK_POLICY_MAX, UNDO_KEEP,
};
pub use confirm::{
    Anchor, ArgLine, ConfirmAnswer, ConfirmDetail, ConfirmEnd, ConfirmId, ConfirmOffer,
    ConfirmParts, ConfirmRequest, Confirmer, EditorRoute, Gesture, GrantScope, NeverOffer,
    SheetLimits, Shown, TaintNote,
};
pub use context::{
    CharRange, ContextKeep, ContextScope, ContextSnapshot, ContextView, EditTarget, EntityLine,
    Here, HereView, Keep, Reveal, Selection, SelectionView, TextPurpose, TextTarget,
    TextTargetView, Visible, VisibleView, WindowPrivacy,
};
pub use derived::{Corrected, Derived, PolicyWriter};
pub use exec_derive::{Derivation, MIN_TOKEN, Served};
pub use exec_facts::{
    DERIVES_OWN, DERIVES_PARAM, DERIVES_READ, ExecFacts, NETWORK_CLOSED, NETWORK_OPEN,
    NETWORK_PARAM, derives_choice, network_choice,
};
pub use exec_reach::{NetAccess, NetReach};
pub use execute::{CannotSandbox, EXECUTE_AS, SandboxState};
pub use external::{ExternalAgent, SheetSurface};
pub use floor::{Floor, FloorEntry, FloorFault, FloorFile, LockedRow};
pub use gate::{CuaAsk, EffectBasis, GateAnswer, NodeFacts, RunMode, WindowClass, WindowTrust};
pub use grant::{
    ActionGrant, ActionGrantKey, GrantCaller, GrantTarget, ProgramName, ProgramNameError,
};
pub use grant_space::{
    Ended, GrantEnd, GrantListFault, KnownSpaces, Reconciled, decode_grants, reconcile,
    without_space,
};
pub use hand::{
    AppSessionId, AppSessionIdError, HandNote, HandTo, SeenCommand, SeenCommandFault, WatchOwner,
    WatchedAgent, WatchedLine, Watching,
};
pub use handover::{HandOver, HandRefusal, HandReply, Handed};
pub use index::{Hit, IndexBatch, IndexEntry, IndexState, SearchAsk, SearchScope, SuggestAsk};
pub use limit::{
    Level, Limit, LimitCall, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitNote, LimitScope,
    Limited, NotATightening, Said, SaidFault, Tighten, UndoOffer, decode_limits, encode_limits,
    limited, limits_with, limits_without,
};
pub use memory_link::{EventSink, LinkFault, MemoryLink, SpaceMemories};
pub use message::{
    Delivery, DraftPart, InboundLine, InboundPart, InboxAsk, MessageDraft, SendRefusal,
};
pub use mode::{DeleteRow, InsideRow, Mode, ModeRows, OtherRow, RowSets, ScreenRow, SendRow};
pub use planner::{
    ActionCard, HandleCard, HandleShape, Held, PlannerView, ReplyFault, StepEnd, StepLine,
    StepShown, TurnIn, TurnSource, TurnVia, UserTurn,
};
pub use preview::{
    FactLine, FileFacts, FileMove, Markdownish, MessageSnip, PageIndex, Preview, TimeRange, size_of,
};
pub use reach_menu::{
    COMPANION_APP, HAND_ON_JOB, PROPOSE_ROUTINE, SKILL_LOAD, TASK_START, offered_to,
};
pub use reader::{
    ReadFault, Reader, ReaderAsk, ReaderError, ReaderTask, SchemaFault, ValueSchema, conforms,
    entity_choice_text,
};
pub use review::{
    AskReason, BreakerTrip, DenyCode, Impact, PolicyId, ReasonCode, ReasonText, ReviewError,
    ReviewMark, Ruling, Stage, Strictness, VerdictKind,
};
pub use roster::{
    EpisodeLine, HelperRosterLine, LeadText, PrimerText, ProfileLine, RecalledLine, RollupLine,
    Roster, RosterDetail, RosterFull, RosterLine, RosterState, SkeletonText, TOLD_LEAD_CHARS,
};
pub use rules::{RuleFile, generated_rules, rule_ids, rule_text, with_rule, without_rule};
pub use run_end::EndWhy;
pub use schema::{OneOf, ToolSchema, tool_schema};
pub use skill::{SKILL_ID_MAX, SkillCard, SkillId, SkillIdError, SkillText, SkillVersion};
pub use space_access::{SpaceAccess, SpaceRefusal, space_access};
pub use spend::{
    Certainty, Spent, TaskTotals, TokenTotals, TurnReport, charge_turn, check_turn, totals_of,
};
pub use standing::{
    AbsPath, AddressFault, CommandFault, CommandPrefix, Cover, Domain, DomainFault, NarrowState,
    PathFault, Recipient, Revocation, RootState, ScopeKind, StandingFileFault, StandingGrant,
    StandingGrantId, StandingIdFault, StandingScope, decode_standing, encode_standing, held_with,
    held_without,
};
pub use standing_match::{ArgFacts, CallFacts, find_standing_for};
pub use standing_offer::{
    AlwaysOffer, AskFacts, BreakerState, BudgetState, Withheld, blocker, holds_standing, may_offer,
    scope_for,
};
pub use started::{NotATerminalScope, StartedFrom, TerminalScope};
pub use summon::{SummonAnswer, SummonOrigin, SummonSerial, VoiceIntent};
pub use task::{
    LedgerStep, SessionOpen, SessionOpened, TaskKind, TaskLedger, TaskStart, close, skeleton_of,
    step_outcome,
};
pub use task_policy::{
    ActionMatch, ArgLabels, Coverage, PolicyChange, Saw, SessionSaw, SinkIntegrity, TaskPolicy,
    TaskPolicyState, TrustedPattern, Widening, compare, covers, covers_reach, intersection,
};
pub use trace::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
pub use undo::{UndoEntry, UndoFault, UndoScope, UndoState};
pub use wire::{
    Displayed, Envelope, GrantAnswer, GrantAsk, HelperNote, IntentsReply, IntentsRequest,
    JournalFilter, NoteAsk, ReadAsk, RecallAsk, RecallView, RecentLine, Resolved, SessionModeAsk,
    SessionNote, StoredAsk, StoredRow, StoredView, UndoReport, WidenAnswer, WidenAsk, WireRefusal,
};
