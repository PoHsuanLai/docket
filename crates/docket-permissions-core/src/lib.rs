//! docket's permission model: how a call is judged, what the person has allowed, and the limits
//! and rules that hold regardless. Strictness and review rulings, the modes and their rows, grants
//! (per action and per Space), standing grants and the offer rule that decides when to offer one,
//! limits, the rule files and the admin floor, the agent's own actions and the commands it runs
//! (`exec_*`, `execute`), and the per-task policy with its trusted patterns and the reach menu.
//!
//! Pure and portable: serde, `bulkhead` for path covers, `url` for host patterns, `docket-vocab`
//! for ids and values, `docket-manifest` for what an action declares, `porter-core` for app names
//! and consent and `prov` for who and what. No clock, no files.

mod agent_app;
mod exec_derive;
mod exec_facts;
mod exec_reach;
mod execute;
mod floor;
mod grant;
mod grant_space;
mod limit;
mod mode;
mod pattern;
mod policy_change;
mod reach_menu;
mod review;
mod rules;
mod space_access;
mod standing;
mod standing_match;
mod standing_offer;
mod task_policy;

pub use agent_app::{
    ACP_AGENT_APP, FILES_READ, FILES_SENSITIVE, FILES_WRITE, PermissionKind, REPORTED,
    TERMINAL_RUN, approves, covers_approval, is_agent_action,
};
pub use agent_app::{action as acp_agent_action, app as acp_agent_app};
pub use exec_derive::{Derivation, MIN_TOKEN, Served};
pub use exec_facts::{
    DERIVES_OWN, DERIVES_PARAM, DERIVES_READ, ExecFacts, NETWORK_CLOSED, NETWORK_OPEN,
    NETWORK_PARAM, derives_choice, network_choice,
};
pub use exec_reach::{NetAccess, NetReach};
pub use execute::{CannotSandbox, EXECUTE_AS, SandboxState};
pub use floor::{Floor, FloorEntry, FloorFault, FloorFile, LockedRow};
pub use grant::{
    ActionGrant, ActionGrantKey, GrantCaller, GrantTarget, ProgramName, ProgramNameError,
};
pub use grant_space::{
    Ended, GrantEnd, GrantListFault, KnownSpaces, Reconciled, decode_grants, reconcile,
    without_space,
};
pub use limit::{
    Level, Limit, LimitCall, LimitCaller, LimitKey, LimitLevel, LimitMaker, LimitNote, LimitScope,
    Limited, NotATightening, Said, SaidFault, Tighten, UndoOffer, decode_limits, encode_limits,
    limited, limits_with, limits_without,
};
pub use mode::{DeleteRow, InsideRow, Mode, ModeRows, OtherRow, RowSets, ScreenRow, SendRow};
pub use policy_change::{PolicyChange, PolicyChangeKind};
pub use reach_menu::{
    COMPANION_APP, HAND_ON_JOB, PROPOSE_ROUTINE, SKILL_LOAD, TASK_START, offered_to,
};
pub use review::{
    AskReason, BreakerTrip, DenyCode, Impact, PolicyId, ReasonCode, ReasonText, ReviewError,
    ReviewMark, Ruling, Stage, Strictness, VerdictKind,
};
pub use rules::{RuleFile, generated_rules, rule_ids, rule_text, with_rule, without_rule};
pub use space_access::{SpaceAccess, SpaceRefusal, space_access};
pub use standing::{
    AddressFault, CommandFault, CommandPrefix, Domain, DomainFault, NarrowState, Recipient,
    Revocation, ScopeKind, StandingFileFault, StandingGrant, StandingGrantId, StandingIdFault,
    StandingScope, decode_standing, encode_standing, held_with, held_without,
};
pub use standing_match::{ArgFacts, CallFacts, find_standing_for};
pub use standing_offer::{
    AlwaysOffer, AskFacts, BreakerState, BudgetState, Withheld, blocker, holds_standing, may_offer,
    scope_for,
};
pub use task_policy::{
    ActionMatch, ArgLabels, Coverage, Saw, SessionSaw, SinkIntegrity, TaskPolicy, TaskPolicyState,
    TrustedPattern, Widening, compare, covers, covers_reach, intersection,
};
