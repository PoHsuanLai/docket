//! The answer object: `/org/quire/Companion1/answer/<task>`.

use docket_core::{
    ActionRef, Args, BudgetKind, CallId, CallRefusal, CallRequest, CardActionId, ConfirmId,
    ContextKeep, DenyCode, EntityRef, LabelText, ParamDecl, Reveal, StepId, TimeRange, UndoId,
};
use porter_core::{AppName, DataClass};
use porter_infer::ServedBy;
use prov::{Effect, TaskId};
use serde::{Deserialize, Serialize};

/// One answer, as it stands.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnswerWire {
    /// The task it belongs to.
    pub task: TaskId,
    /// Where it is.
    pub phase: AnswerPhase,
    /// What it shows.
    pub body: AnswerBody,
    /// What it was made of.
    pub footer: FooterWire,
}

/// Where an answer is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AnswerPhase {
    /// Working.
    Thinking,
    /// Words are arriving.
    Streaming,
    /// Waiting for the person.
    NeedsYou(NeedsYou),
    /// Finished.
    Done,
    /// Could not finish.
    Failed,
    /// Stopped.
    Cancelled,
}

/// What the person is needed for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum NeedsYou {
    /// A confirmation on the sheet. A spoken answer never answers it.
    Confirm(ConfirmId),
    /// A parameter to fill.
    Form(FormWire),
    /// A question with choices.
    Question {
        /// The question.
        text: String,
        /// The choices.
        choices: Vec<String>,
    },
}

/// What an answer shows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum AnswerBody {
    /// Text; handles are resolved for the screen by `Session.Display`, never by a model.
    Text {
        /// The lines.
        lines: Vec<Reveal<String>>,
    },
    /// A reply draft.
    DraftReply {
        /// The recipients.
        to: Vec<EntityRef>,
        /// The subject.
        subject: Reveal<String>,
        /// The body.
        body: Reveal<String>,
        /// What the person may do with it.
        actions: Vec<CardWire>,
    },
    /// A proposed event.
    ProposedEvent {
        /// The title.
        title: String,
        /// When.
        when: TimeRange,
        /// Who.
        people: Vec<EntityRef>,
        /// What the person may do with it.
        actions: Vec<CardWire>,
    },
    /// A plan.
    Plan(PlanWire),
    /// An inline replacement in the person's field: the change is shown and waits for Apply.
    Replace {
        /// What is there.
        original: Reveal<String>,
        /// What is proposed.
        proposed: Reveal<String>,
        /// The journal row once applied.
        undo: Option<UndoId>,
    },
    /// A form.
    Form(FormWire),
    /// A refusal.
    Refused(RefusalWire),
}

/// A plan: steps that run at once, each confirmed on its own where it must be.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanWire {
    /// The steps.
    pub steps: Vec<PlanStepWire>,
}

/// One step of a plan.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanStepWire {
    /// Its number.
    pub id: StepId,
    /// The action.
    pub action: ActionRef,
    /// The manifest's label.
    pub label: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// Where it is.
    pub state: StepWireState,
    /// The call, once made.
    pub call: Option<CallId>,
}

/// Where a plan step is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum StepWireState {
    /// Not yet.
    Pending,
    /// Running.
    Running,
    /// Done, with its journal row if undoable.
    Done {
        /// The row.
        undo: Option<UndoId>,
    },
    /// Failed or refused.
    Failed(CallRefusal),
    /// Skipped.
    Skipped,
    /// Undone.
    Undone,
}

/// One action on a card: taking it becomes a `Run.Perform`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CardWire {
    /// Its id.
    pub id: CardActionId,
    /// The label.
    pub label: LabelText,
    /// Its effect.
    pub effect: Effect,
    /// The call it makes.
    pub call: CallRequest,
}

/// A form to fill, from an app's `NeedsParam`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FormWire {
    /// The action it is for.
    pub action: ActionRef,
    /// The parameters.
    pub params: Vec<ParamDecl>,
    /// The values so far.
    pub values: Args,
}

/// Why nothing happened, in the person's terms.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum RefusalWire {
    /// The data may not leave this computer and no model here can do it.
    NeedsCloud(DataClass),
    /// Not allowed.
    NotAllowed(DenyCode),
    /// No typed action and computer use is off for the app.
    NoWay {
        /// The app.
        app: AppName,
    },
    /// A budget ran out.
    OverBudget(BudgetKind),
    /// It failed.
    Failed(String),
}

/// What an answer was made of.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FooterWire {
    /// The models that answered and where they ran.
    pub served: Vec<ServedBy>,
    /// The things it drew on.
    pub sources: Vec<EntityRef>,
    /// The chips of context the person kept.
    pub keep: ContextKeep,
}
