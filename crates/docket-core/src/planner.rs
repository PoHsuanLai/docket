//! The planner's contract: what it may see, built only by the router's `planner_view`. The
//! planner reads trusted text; everything untrusted reaches it as a [`Handle`].

use crate::call::CallRefusal;
use crate::confirm::ConfirmEnd;
use crate::context::{ContextKeep, ContextView, Reveal};
use crate::ids::{ActionRef, CallId, Handle, LabelText, TurnId, UndoId};
use crate::manifest::{AgentReach, Lasting};
use crate::message::InboundLine;
use crate::roster::{EpisodeLine, PrimerText, ProfileLine, RecalledLine, RollupLine, Roster};
use crate::schema::ToolSchema;
use crate::task_policy::TaskPolicy;
use crate::units::CharCount;
use crate::value::Value;
use prov::{Effect, EntityKind, Integrity, Source, UnixSeconds};
use serde::{Deserialize, Serialize};

/// How a turn reached the router. Informational: it grants nothing, and integrity comes from
/// [`TurnSource`] as for typed text. The planner may confirm odd names; memory says "you said".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnVia {
    /// Typed.
    Typed,
    /// Spoken through the hold gesture and transcribed on this computer.
    Spoken,
}

/// Where a turn was recorded from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TurnSource {
    /// The launcher.
    Launcher,
    /// A prompt field inside this app: the task policy is capped to that app plus reads, and
    /// widening confirms.
    Field(porter_core::AppName),
}

/// One prompt, as the router records it. Only the launcher and field roles may record a turn.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TurnIn {
    /// What the person said.
    pub text: String,
    /// Where it was said.
    pub origin: crate::call::Origin,
    /// The context chips they kept.
    pub keep: ContextKeep,
    /// Typed or spoken.
    pub via: TurnVia,
}

/// A recorded turn: the person's own words, trusted, verbatim.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserTurn {
    /// Its id.
    pub id: TurnId,
    /// What the person said.
    pub text: String,
    /// When.
    pub at: UnixSeconds,
    /// Where it was recorded.
    pub from: TurnSource,
    /// Typed or spoken.
    pub via: TurnVia,
}

macro_rules! redact_turn {
    ($t:ident) => {
        // The words are the person's: Debug shows the length only.
        impl std::fmt::Debug for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, concat!(stringify!($t), "(<{} bytes>)"), self.text.len())
            }
        }
    };
}
redact_turn!(TurnIn);
redact_turn!(UserTurn);

/// One action as the planner is offered it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionCard {
    /// Which action.
    pub action: ActionRef,
    /// What the person reads.
    pub label: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// The schema of its arguments.
    pub tool: ToolSchema,
    /// Whether it is offered or asks every time.
    pub reach: AgentReach,
    /// Whether it writes lasting memory.
    pub lasting: Lasting,
}

/// What a handle holds, in shape only.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum HandleShape {
    /// Text.
    Text,
    /// A thing of this kind.
    Entity(EntityKind),
    /// A file.
    File,
}

/// A value the planner may name but not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HandleCard {
    /// The handle.
    pub handle: Handle,
    /// What it holds.
    pub shape: HandleShape,
    /// Where it came from.
    pub from: Source,
    /// How long it is.
    pub size: CharCount,
}

/// How much of a past step the planner is shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StepShown {
    /// In full.
    Full,
    /// Masked to one line (`[outcome: archived 12 messages, undo #41]`), which keeps the cached
    /// prefix stable.
    Masked,
}

/// How a past step ended, as the planner sees it: codes only, never a policy id or a reviewer's
/// words.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StepEnd {
    /// It ran.
    Done {
        /// The app's words.
        said: Option<LabelText>,
        /// The value, with text as handles.
        value: Option<Reveal<Value>>,
        /// The journal row, if undoable.
        undo: Option<UndoId>,
    },
    /// It was refused.
    Refused(CallRefusal),
    /// The person did not confirm it.
    Unconfirmed(ConfirmEnd),
}

/// One call of the session, as history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepLine {
    /// The call.
    pub call: CallId,
    /// The action.
    pub action: ActionRef,
    /// Its effect.
    pub effect: Effect,
    /// How it ended.
    pub end: StepEnd,
    /// How much to show.
    pub shown: StepShown,
}

/// Everything the planner may see; built only by the router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannerView {
    /// The person's own words: trusted.
    pub turns: Vec<UserTurn>,
    /// Where the person is, untrusted text as handles.
    pub context: ContextView,
    /// What the planner may call.
    pub actions: Vec<ActionCard>,
    /// What it may name but not read.
    pub handles: Vec<HandleCard>,
    /// The session's calls and how they ended.
    pub history: Vec<StepLine>,
    /// The planner's integrity as the router sees it.
    pub taint: Integrity,
    /// What this task may do, if the writer produced a policy.
    pub task_policy: Option<TaskPolicy>,
    /// The Space's primer: trusted facts only.
    pub primer: Option<PrimerText>,
    /// Pinned desktop-scope facts the person stated.
    pub profile: Vec<ProfileLine>,
    /// The latest day or week digest line.
    pub rollup: Option<RollupLine>,
    /// Who else is working, one line each.
    pub roster: Roster,
    /// What recently ended in this Space.
    pub episodes: Vec<EpisodeLine>,
    /// What recall brought up for this turn.
    pub recalled: Vec<RecalledLine>,
    /// Messages that landed for this task since the last turn. Input only: nothing in them
    /// widens the task, and untrusted words are handles.
    pub inbox: Vec<InboundLine>,
}
