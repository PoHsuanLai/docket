//! docket's vocabulary, and the pure behaviour that is cheap to build with it: the manifest and
//! its validation, values and their schemas, context and previews, calls and every way they end,
//! the undo journal, confirmation, budgets and the kill switch, the per-task policy, the
//! planner and reader contract, the roster, messages, tasks and their episodes, audit records,
//! the proposed configuration and the Intents1 wire.
//!
//! Pure and portable: serde only. Names `prov` for who and what, `almanac-core` for the pure
//! shapes of memory (episodes, recall, recent), `cua-action` for the computer-use gate and
//! `model-provider` for the structured-output shapes `ValueSchema` renders through.

mod agent_app;
mod args;
mod atomic_file;
mod audit;
mod budget;
mod call;
mod caller;
mod classify;
mod config;
mod confirm;
mod context;
mod derived;
mod exec_derive;
mod exec_facts;
mod exec_reach;
mod execute;
mod external;
mod gate;
mod grant;
mod grant_space;
mod ids;
mod index;
mod manifest;
mod marks;
mod message;
mod pattern;
mod planner;
mod preview;
mod reader;
mod review;
mod roster;
mod schema;
mod skill;
mod space_access;
mod standing;
mod standing_match;
mod standing_offer;
mod started;
mod summon;
mod task;
mod task_policy;
mod trace;
mod undo;
mod units;
mod validate;
mod value;
pub mod when;
mod wire;
mod workspace;

pub use agent_app::{
    ACP_AGENT_APP, FILES_READ, FILES_SENSITIVE, FILES_WRITE, PermissionKind, REPORTED,
    TERMINAL_RUN, approves, covers_approval, is_agent_action,
};
pub use agent_app::{action as acp_agent_action, app as acp_agent_app};
pub use args::{ArgsFault, TARGET_KEY, TargetFault, Why, args_from_json, target_from_json};
pub use atomic_file::{read_optional, write_atomic};
pub use audit::{AuditRecord, ConfirmAnswerKind, DecidedBy, PolicyChangeKind};
pub use budget::{
    Budget, BudgetKind, Cost, Halt, HaltCause, KillSwitch, Ledger, Reviewed, charge, halted,
};
pub use call::{
    ActivatedInvocation, ActivationToken, AppRefusal, ArgFault, CallEnd, CallProgress, CallRefusal,
    CallRequest, FailText, Follow, Invocation, Origin, Outcome, Undoable,
};
pub use caller::{CallerId, CallerRole, Member, is_terminal_scope};
pub use classify::{
    CallClass, Classification, ClassifyAnswer, ClassifyFault, DelegationEnd, PerCall,
};
pub use config::{
    AgentConfig, AssemblerBudget, BreakerLimits, IdleRules, ReviewTimeouts, SETTING_ROWS,
    SettingRow, SettingValue,
};
pub use confirm::{
    Anchor, ArgLine, ConfirmAnswer, ConfirmDetail, ConfirmEnd, ConfirmId, ConfirmOffer,
    ConfirmParts, ConfirmRequest, Confirmer, EditorRoute, Gesture, GrantScope, Shown, TaintNote,
};
pub use context::{
    CharRange, ContextKeep, ContextScope, ContextSnapshot, ContextView, EditTarget, EntityLine,
    Here, HereView, Keep, Reveal, Selection, SelectionView, TextPurpose, TextTarget,
    TextTargetView, Visible, VisibleView, WindowPrivacy,
};
pub use derived::{Corrected, Derived};
pub use exec_derive::{Derivation, MIN_TOKEN, Served};
pub use exec_facts::{
    DERIVES_OWN, DERIVES_PARAM, DERIVES_READ, ExecFacts, NETWORK_CLOSED, NETWORK_OPEN,
    NETWORK_PARAM, derives_choice, network_choice,
};
pub use exec_reach::{NetAccess, NetReach};
pub use execute::{CannotSandbox, EXECUTE_AS, SandboxState};
pub use external::{ExternalAgent, SheetSurface};
pub use gate::{CuaAsk, EffectBasis, GateAnswer, NodeFacts, RunMode, WindowClass, WindowTrust};
pub use grant::{
    ActionGrant, ActionGrantKey, GrantCaller, GrantTarget, ProgramName, ProgramNameError,
};
pub use grant_space::{
    Ended, GrantEnd, GrantListFault, KnownSpaces, Reconciled, decode_grants, reconcile,
    without_space,
};
pub use ids::{
    ActionRef, CallId, CardActionId, ChoiceId, EntityRef, FileRef, Handle, IconName, IntentsVocab,
    LabelText, ParamName, RelationName, StepId, TextTargetRef, TurnId, UndoId, UndoToken,
    UtteranceId, ViewName, WindowKey, action_prefix,
};
pub use index::{Hit, IndexBatch, IndexEntry, IndexState, SearchAsk, SearchScope, SuggestAsk};
pub use manifest::{
    ActionDecl, AgentReach, ArgSink, Cardinality, DryRun, EntityDecl, IndexPolicy, KeyHint,
    Lasting, Latency, Manifest, ParamDecl, ParamNeed, PropDecl, RELATION_ARG, RelationDecl,
    ResultShape, TargetKind, TitleTrust, UndoSupport, Visibility, related_name,
};
pub use marks::{hides, plain_text, reorders};
pub use message::{
    Delivery, DraftPart, InboundLine, InboundPart, InboxAsk, MessageDraft, SendRefusal,
};
pub use planner::{
    ActionCard, HandleCard, HandleShape, Held, PlannerView, ReplyFault, StepEnd, StepLine,
    StepShown, TurnIn, TurnSource, TurnVia, UserTurn,
};
pub use preview::{
    FactLine, FileFacts, FileMove, Markdownish, MessageSnip, PageIndex, Preview, TimeRange, size_of,
};
pub use reader::{
    ReadFault, Reader, ReaderAsk, ReaderError, ReaderTask, SchemaFault, ValueSchema, conforms,
    entity_choice_text,
};
pub use review::{
    AskReason, BreakerTrip, DenyCode, Impact, PolicyId, ReasonCode, ReasonText, ReviewError,
    ReviewMark, Ruling, ShadowMode, Stage, Strictness, VerdictKind,
};
pub use roster::{
    EpisodeLine, LeadText, PrimerText, ProfileLine, RecalledLine, RollupLine, Roster, RosterDetail,
    RosterFull, RosterLine, RosterState, SkeletonText, TOLD_LEAD_CHARS,
};
pub use schema::{ToolSchema, tool_schema};
pub use skill::{SKILL_ID_MAX, SkillCard, SkillId, SkillIdError, SkillText, SkillVersion};
pub use space_access::{SpaceAccess, SpaceRefusal, space_access};
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
    ActionMatch, ArgLabels, Coverage, PolicyChange, PolicyWriter, Saw, SessionSaw, SinkIntegrity,
    TaskPolicy, TaskPolicyState, TrustedPattern, Widening, compare, covers, intersection,
};
pub use trace::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
pub use undo::{UndoEntry, UndoFault, UndoScope, UndoState};
pub use units::{CharCount, Depth, Generation, Millis, Scale, Seconds};
pub use validate::{ManifestError, ValidManifest, fits, open_name, validate};
pub use value::{Args, ChoiceDecl, CivilDate, Decimal, Lines, ParamType, TargetValue, Value};
pub use wire::{
    Envelope, GrantAnswer, GrantAsk, IntentsReply, IntentsRequest, JournalFilter, NoteAsk,
    NoteSlug, NoteSlugError, ReadAsk, RecallAsk, RecallView, RecentLine, Resolved, SessionNote,
    StoredAsk, StoredRow, StoredView, UndoReport, WidenAnswer, WidenAsk, WireRefusal,
};
pub use workspace::{Workspace, WorkspaceError};
