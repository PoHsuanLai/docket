//! docket's vocabulary above `docket-vocab`, `docket-manifest`, `docket-permissions-core` and
//! `companion-model`, and the pure behaviour that is cheap to build with it: context and previews,
//! calls and every way they end, the undo journal, confirmation and the brief, budgets, spending
//! and the kill switch, the planner and reader contract, the roster, messages, restore points,
//! audit records and the proposed configuration, and the memory and event-log seams. Ids, units,
//! values, call requests, dates and the `Clock` seam live in `docket-vocab`; the manifest and its
//! argument reader in `docket-manifest`; grants, limits and the task policy in
//! `docket-permissions-core`; the helpers and routines in `companion-model`. The Intents1 wire is in
//! `docket-wire`, and the computer-use gate's ask and answer in `docket-gate-wire`, both above this
//! crate.
//!
//! Pure and portable: serde only. Names `prov` for who and what, `porter-core` for accounts and
//! labels, `almanac-core` for the pure shapes of memory (episodes, recall, recent) and
//! `model-provider` for the structured-output shapes `ValueSchema` renders through.

mod audit;
mod brief;
mod budget;
mod call;
mod checkpoint;
mod compact;
mod config;
mod confirm;
mod context;
mod derived;
mod external;
mod handover;
mod index;
mod memory_link;
mod message;
mod planner;
mod preview;
mod reader;
mod roster;
mod run_end;
mod schema;
mod skill;
mod spend;
mod trace;
mod undo;

pub use audit::{AuditRecord, ConfirmAnswerKind, DecidedBy};
pub use brief::{
    BRIEF_MAX, BriefEdit, BriefFault, BriefFlag, BriefSheet, BriefText, BriefWhy,
    FROM_CHECKED_BRIEF, flag_brief, from_checked_brief,
};
pub use budget::{Budget, BudgetKind, Cost, Halt, HaltCause, KillSwitch, Ledger, charge, halted};
pub use call::{
    ActivatedInvocation, ActivationToken, AppRefusal, ArgFault, CallEnd, CallProgress, CallRefusal,
    FailText, Follow, Invocation, Outcome, Undoable,
};
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
pub use external::{ExternalAgent, SheetSurface};
pub use handover::{HandOver, HandRefusal, HandReply, Handed};
pub use index::{Hit, IndexBatch, IndexEntry, IndexState, SearchAsk, SearchScope, SuggestAsk};
pub use memory_link::{EventSink, LinkFault, MemoryLink, SpaceMemories};
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
pub use roster::{
    EpisodeLine, HelperRosterLine, LeadText, PrimerText, ProfileLine, RecalledLine, RollupLine,
    Roster, RosterDetail, RosterFull, RosterLine, RosterState, SkeletonText, TOLD_LEAD_CHARS,
};
pub use run_end::EndWhy;
pub use schema::{OneOf, ToolSchema, tool_schema};
pub use skill::{SKILL_ID_MAX, SkillCard, SkillId, SkillIdError, SkillText, SkillVersion};
pub use spend::{
    Certainty, Spent, TaskTotals, TokenTotals, TurnReport, charge_turn, check_turn, totals_of,
};
pub use trace::{ExchangeAnswer, ExchangeCall, ExchangeMessage, ModelExchange};
pub use undo::{UndoEntry, UndoFault, UndoScope, UndoState};
