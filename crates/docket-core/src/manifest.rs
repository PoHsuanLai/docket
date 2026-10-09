//! The app manifest: `$XDG_DATA_DIRS/quire/intents/<AppName>.toml`, installed beside the app's
//! `dbus-1/services/<AppName>.service`. Its serde form is the file format and every field is
//! written (porter's provider-file rule). TOML is parsed in `docket-router::registry`; this
//! crate never reaches `toml`.

use crate::classify::PerCall;
use crate::ids::{IconName, IntentsVocab, LabelText, ParamName, RelationName};
use crate::value::{ChoiceDecl, ParamType, Value};
use porter_core::DataClass;
use prov::{ActionName, Effect, EntityKind, Source};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// One app's declaration of what it owns and what it can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    /// The vocabulary the file is written in.
    pub vocab: IntentsVocab,
    /// The app.
    pub app: porter_core::AppName,
    /// The kinds of thing it owns.
    pub entities: Vec<EntityDecl>,
    /// What it can do.
    pub actions: Vec<ActionDecl>,
    /// Who may be shown its actions. Absent from a file means `everyone`; it is not written when
    /// it is.
    #[serde(default, skip_serializing_if = "Visibility::is_everyone")]
    pub visibility: Visibility,
}

/// Who is shown an app's actions as things to call. It is a matter of what is listed, not of
/// what is allowed: the router refuses a call whatever the listing says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    /// Every agent and client that may use the app.
    #[default]
    Everyone,
    /// Only the host the app stands for (the external agent's host): no planner, MCP client or
    /// policy writer of an ordinary session is told its actions exist.
    HostOnly,
}

impl Visibility {
    /// Whether this is the default, so the file need not say it.
    pub fn is_everyone(&self) -> bool {
        *self == Self::Everyone
    }
}

/// One kind of thing an app owns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDecl {
    /// Its kind.
    pub kind: EntityKind,
    /// Singular, for the UI.
    pub label: LabelText,
    /// Plural, for the UI.
    pub plural: LabelText,
    /// Its icon.
    pub icon: IconName,
    /// The data class its content belongs to.
    pub class: DataClass,
    /// Whether the app pushes it into the router's index.
    pub index: IndexPolicy,
    /// Who writes this kind's titles.
    pub titles: TitleTrust,
    /// The properties the context call may report.
    pub props: Vec<PropDecl>,
    /// The things of other kinds this kind's things are related to, which the app can name
    /// (`[[entities.relations]]`). Absent from a file means none; it is not written when none.
    /// Each makes the kind's related action (`<kind>.related`, see [`related_name`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<RelationDecl>,
}

/// How many things a relation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Cardinality {
    /// At most one (a message's sender).
    One,
    /// Any number (an event's attendees).
    Many,
}

/// One relation of an entity kind: the app resolves it, for a thing of the kind, to things of
/// another kind. The planner never reads untrusted content, so this is how it gets, as a handle,
/// the sender of a thread or the owner of a file, to use where an action takes a thing of that
/// kind. Resolving is a read of the kind's related action, so it goes through the whole gate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelationDecl {
    /// Its name, one of the kind's `relation` choices.
    pub name: RelationName,
    /// What the person reads.
    pub label: LabelText,
    /// The kind of thing it names.
    pub to: EntityKind,
    /// One thing or several.
    pub many: Cardinality,
    /// Who decides what it names. `third_party` when the answer comes from content somebody else
    /// wrote (the sender of a message): the things it names are then untrusted whatever the app
    /// says, and so is anything chosen from them. `app_authored` when the app's own data decides.
    pub trust: TitleTrust,
}

/// The name of the read action that resolves the relations of `kind` (`mail.thread` resolves by
/// `mail.thread.related`). The router derives it from the declared relations; a manifest never
/// declares it itself.
pub fn related_name(kind: &EntityKind) -> Option<ActionName> {
    ActionName::parse(&format!("{}.related", kind.as_str())).ok()
}

/// The name of the argument of a related action that picks the relation.
pub const RELATION_ARG: &str = "relation";

/// Whether a kind is pushed into the shadow index.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IndexPolicy {
    /// Searched live through the app.
    NotIndexed,
    /// Pushed to the router, per app opt-out.
    Indexed,
}

/// Whether a text came from the app or from someone else through it.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TitleTrust {
    /// The app wrote it (a folder name, a setting).
    AppAuthored,
    /// Somebody else wrote it (a mail subject is `ThirdParty(Mail)`).
    ThirdParty(Source),
}

/// One property an entity kind exposes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PropDecl {
    /// Its name.
    pub name: ParamName,
    /// What the person reads.
    pub label: LabelText,
    /// Its type.
    pub ty: ParamType,
}

/// One thing an app can do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionDecl {
    /// Its name; it starts with the app's own prefix (`mail.thread.archive`).
    pub name: ActionName,
    /// What the person reads ("Archive").
    pub label: LabelText,
    /// What it acts on.
    pub on: TargetKind,
    /// What it takes.
    pub params: Vec<ParamDecl>,
    /// What it can do to the world. With `per_call = "classified"` this is a ceiling.
    pub effect: Effect,
    /// Whether the provider classifies each call below the declared effect (`Classify`). Absent
    /// from a file means `declared`; it is not written when it is.
    #[serde(default, skip_serializing_if = "PerCall::is_declared")]
    pub per_call: PerCall,
    /// The data classes it touches.
    pub classes: BTreeSet<DataClass>,
    /// Whether it can be taken back.
    pub undo: UndoSupport,
    /// Whether an agent may see it.
    pub reach: AgentReach,
    /// How long it takes, which sets the router's timeout.
    pub latency: Latency,
    /// What it returns.
    pub result: ResultShape,
    /// A key chord the launcher may show.
    pub keys: KeyHint,
    /// Whether it writes lasting agent memory.
    pub lasting: Lasting,
    /// Whether the app can describe the change before making it.
    pub dry_run: DryRun,
}

/// What an action acts on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum TargetKind {
    /// Nothing in particular.
    Nothing,
    /// One thing of a kind.
    One(EntityKind),
    /// Several things of a kind.
    Many(EntityKind),
    /// A text field.
    Text,
    /// Files.
    Files,
}

/// One parameter of an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ParamDecl {
    /// Its name.
    pub name: ParamName,
    /// What the person reads.
    pub label: LabelText,
    /// Its type.
    pub ty: ParamType,
    /// Whether it must be given.
    pub need: ParamNeed,
    /// Where its value goes: the router gates arguments by the integrity of the sinks they feed.
    pub sink: ArgSink,
}

/// Whether a parameter must be given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ParamNeed {
    /// The call fails without it (`NeedsParam`).
    Required,
    /// May be left out.
    Optional,
    /// Left out means this.
    Defaulted(Value),
}

/// Where an argument's value goes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ArgSink {
    /// Nowhere that matters (a label, a count).
    Inert,
    /// Who something is sent to.
    Recipient,
    /// A place something is sent (a URL, an account).
    Destination,
    /// The content that is sent or written.
    Body,
    /// A file path that is written or removed.
    Path,
    /// A query that leaves the machine.
    Query,
}

/// Whether an action's effect can be taken back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UndoSupport {
    /// The app returns an undo token.
    Token,
    /// It cannot be taken back.
    NotUndoable,
}

/// Whether an agent may see and call an action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentReach {
    /// Only the person, through the app.
    Hidden,
    /// An agent may offer it.
    Offered,
    /// An agent may offer it, and the person is always asked.
    AskAlways,
}

/// How long an action takes: the router's timeouts are 250 ms, 5 s, and a progress request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Latency {
    /// Answers at once.
    Instant,
    /// Answers within seconds.
    Quick,
    /// Takes long enough to report progress.
    Long,
}

/// What an action returns.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum ResultShape {
    /// Nothing.
    Nothing,
    /// A value, with a trust floor for returned text.
    Value {
        /// Its type.
        ty: ParamType,
        /// Who wrote text in it.
        trust: TitleTrust,
    },
    /// Things of a kind.
    Entities(EntityKind),
}

/// A key chord an app suggests for an action.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "v", rename_all = "snake_case")]
pub enum KeyHint {
    /// None.
    None,
    /// This chord, as sill parses it.
    Chord(String),
}

/// Whether an action writes lasting agent memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lasting {
    /// No.
    No,
    /// Yes: a fact the companion keeps.
    AgentMemory,
    /// Untrusted input lands in the pending queue (`memory.propose`); the queue is the
    /// confirmation.
    Staged,
}

/// Whether the app can describe a change before it makes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DryRun {
    /// No; the confirmation shows the router's own formatting of the arguments.
    None,
    /// `IntentProvider1.DryRun` answers a `Preview`.
    Preview,
}

impl EntityDecl {
    /// The read action that resolves this kind's relations, derived from them: one thing of the
    /// kind in, the `relation` to follow as a choice, the things it names out. `None` when the
    /// kind declares no relation. `classes` is the kind's data class and the class of every
    /// kind in `others` that a relation names, so the gate asks for what resolving reveals.
    pub fn related_action(&self, others: &[EntityDecl]) -> Option<ActionDecl> {
        let first = self.relations.first()?;
        let name = related_name(&self.kind)?;
        let options = self
            .relations
            .iter()
            .map(|r| {
                Some(ChoiceDecl {
                    id: r.name.choice()?,
                    label: r.label.clone(),
                })
            })
            .collect::<Option<Vec<_>>>()?;
        let mut classes = BTreeSet::from([self.class]);
        classes.extend(
            others
                .iter()
                .filter(|o| self.relations.iter().any(|r| r.to == o.kind))
                .map(|o| o.class),
        );
        Some(ActionDecl {
            name,
            label: LabelText::parse("Find related").ok()?,
            on: TargetKind::One(self.kind.clone()),
            params: vec![ParamDecl {
                name: ParamName::parse(RELATION_ARG).ok()?,
                label: LabelText::parse("Which related thing").ok()?,
                ty: ParamType::Choice(options),
                need: ParamNeed::Required,
                sink: ArgSink::Inert,
            }],
            effect: Effect::Read,
            per_call: PerCall::default(),
            classes,
            undo: UndoSupport::NotUndoable,
            reach: AgentReach::Offered,
            latency: Latency::Quick,
            result: ResultShape::Entities(first.to.clone()),
            keys: KeyHint::None,
            lasting: Lasting::No,
            dry_run: DryRun::None,
        })
    }

    /// The relation of this kind named `name`.
    pub fn relation(&self, name: &RelationName) -> Option<&RelationDecl> {
        self.relations.iter().find(|r| &r.name == name)
    }
}

impl Manifest {
    /// The kind whose relations `action` resolves, if `action` is a derived related action.
    pub fn related_kind(&self, action: &ActionName) -> Option<&EntityDecl> {
        self.entities
            .iter()
            .find(|e| !e.relations.is_empty() && related_name(&e.kind).as_ref() == Some(action))
    }
}
