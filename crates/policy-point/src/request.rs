//! What the router asks the policy point: a principal, an operation, the action, and the
//! context that Cedar's rules read. Every context field has a slug form in the schema.

use docket_core::BudgetKind;
use docket_permissions_core::{Impact, Saw, SessionSaw, SinkIntegrity};
use docket_vocab::Origin;
use porter_core::consent::Usage;
use porter_core::{AppName, Count, DataClass, Isolation};
use prov::{ActionName, ActorKind, Confidentiality, Effect, Integrity, SpaceId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Who asks.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalFacts {
    /// The kind of actor.
    pub kind: ActorKind,
    /// The app the caller is, when it is one.
    pub caller: AppName,
    /// How far the name can be trusted.
    pub isolation: Isolation,
}

/// What is asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Op {
    /// Perform an action.
    Perform,
    /// Search.
    Search,
    /// Preview.
    Preview,
    /// Suggest parameter options.
    Suggest,
    /// Read context.
    ReadContext,
    /// Undo.
    Undo,
    /// Push to the index.
    IndexPush,
}

/// What the action is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionFacts {
    /// The app that declares it.
    pub app: AppName,
    /// Its name.
    pub action: ActionName,
    /// Its effect.
    pub effect: Effect,
    /// The data classes it touches.
    pub classes: BTreeSet<DataClass>,
    /// Whether an agent may see it.
    pub reach: docket_manifest::AgentReach,
    /// Whether it writes lasting memory.
    pub lasting: docket_manifest::Lasting,
}

/// Where the target is relative to the session's Space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpaceRelation {
    /// The same Space.
    Same,
    /// Another one.
    Other,
    /// Not bound to any.
    Unbound,
}

/// What standing consent says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GrantState {
    /// An `Always` grant covers it.
    Always,
    /// Ask once.
    Once,
    /// No grant yet.
    None,
    /// Denied.
    Denied,
}

/// Whether the call is inside the task policy: Cedar's two-state view of `Coverage`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageState {
    /// Inside.
    Inside,
    /// Outside.
    Outside,
}

impl From<&docket_permissions_core::Coverage> for CoverageState {
    fn from(coverage: &docket_permissions_core::Coverage) -> Self {
        match coverage {
            docket_permissions_core::Coverage::Inside => CoverageState::Inside,
            docket_permissions_core::Coverage::Outside(_) => CoverageState::Outside,
        }
    }
}

/// Whether the person gave a standing grant for this action from the terminal ("allow from the
/// terminal: this action, until logout"). The router reads it from the terminal session's task
/// policy; only a `quire-do` call has one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TerminalGrant {
    /// The person gave one.
    Granted,
    /// They did not (and every call that is not from the terminal).
    NotGranted,
}

/// Whether the kill switch has stopped the call's Space.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltFact {
    /// Running.
    #[serde(rename = "none")]
    Running,
    /// The Space, or everything, is halted.
    Halted,
}

/// Whether the call fits the session's budgets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetFact {
    /// It fits.
    Within,
    /// This budget would be over.
    Over(BudgetKind),
}

/// Whether the exact call was denied before.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatFact {
    /// New.
    Fresh,
    /// Denied earlier, with the same arguments.
    Repeated,
}

/// The strongest limit that covers the call: the person's, the assistant's or the
/// administrator's floor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LimitFact {
    /// None covers it.
    #[serde(rename = "none")]
    Free,
    /// It always asks.
    Ask,
    /// It is refused without asking.
    Never,
}

/// Whether the session read untrusted content before a call that changes something. Reading
/// stays clean, or one mail read would put a question on every read after.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaintFact {
    /// Nothing untrusted was read, or the call only reads.
    Clean,
    /// Untrusted content was read and this call changes something.
    Tainted,
}

/// Whether the person already allowed the screen run this step belongs to: by the sheet that
/// opened the run ("Let it use this app"), or by a standing grant for the app. Every call that
/// is not a screen step is `NotAsked`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunFact {
    /// The person said yes to this run.
    Allowed,
    /// Nobody asked the person about a run (the call is not a step of one, or it was not allowed).
    NotAsked,
}

/// Everything about the call that Cedar's rules read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyContext {
    /// The session's Space.
    pub space: SpaceId,
    /// Where the target is.
    pub target_space: SpaceRelation,
    /// The integrity of the arguments.
    pub args: Integrity,
    /// The planner's integrity.
    pub planner: Integrity,
    /// How confidential what the call carries is.
    pub confidentiality: Confidentiality,
    /// Interactive or background.
    pub usage: Usage,
    /// Where the call came from.
    pub origin: Origin,
    /// How many things it touches.
    pub count: Count,
    /// More than this many asks (`agent.mass_at`, proposed 20).
    pub mass_at: Count,
    /// What standing consent says.
    pub grant: GrantState,
    /// Whether the terminal holds a standing grant for this action.
    pub terminal: TerminalGrant,
    /// Whether the call is inside the task policy.
    pub coverage: CoverageState,
    /// The task policy's effect ceiling.
    pub task_ceiling: Effect,
    /// What the session has seen, computed by the router.
    pub saw: SessionSaw,
    /// The integrity of each sink.
    pub sinks: SinkIntegrity,
    /// How much is at stake.
    pub impact: Impact,
    /// Whether the call's Space is halted.
    pub halt: HaltFact,
    /// Whether the call fits the budgets.
    pub budget: BudgetFact,
    /// Whether the exact call was denied before.
    pub repeat: RepeatFact,
    /// What the limits say.
    pub limit: LimitFact,
    /// Whether the session is tainted for this call.
    pub taint: TaintFact,
    /// Whether the person allowed the screen run this step belongs to.
    pub run: RunFact,
    /// The mode of the conversation the call belongs to.
    pub mode: docket_permissions_core::Mode,
    /// What that mode does without asking, from the settings and the administrator's floor.
    pub rows: docket_permissions_core::ModeRows,
}

/// One question for the policy point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyRequest {
    /// Who asks.
    pub principal: PrincipalFacts,
    /// What is asked.
    pub op: Op,
    /// The action.
    pub resource: ActionFacts,
    /// What the rules read.
    pub context: PolicyContext,
}

impl PolicyContext {
    /// Whether the session has seen both private data and untrusted input: two legs of the Rule
    /// of Two.
    pub fn two_legs(&self) -> bool {
        self.saw.private == Saw::Seen && self.saw.untrusted == Saw::Seen
    }
}
