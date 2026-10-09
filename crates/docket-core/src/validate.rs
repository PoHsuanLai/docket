//! Manifest validation: pure, total, one error per rule a caller can act on.

use crate::ids::{IntentsVocab, ParamName, RelationName, action_prefix};
use crate::manifest::{
    ActionDecl, ArgSink, IndexPolicy, Manifest, ParamNeed, TargetKind, UndoSupport,
};
use crate::value::{ParamType, Value};
use prov::{ActionName, Effect, EntityKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// Why a manifest is not valid.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ManifestError {
    /// The file is written in a newer vocabulary than this build speaks.
    #[error("vocabulary {0:?} is newer than this build")]
    VocabTooNew(IntentsVocab),
    /// Two actions share a name.
    #[error("action {0} is declared twice")]
    DuplicateAction(ActionName),
    /// Two entity declarations share a kind.
    #[error("kind {0} is declared twice")]
    DuplicateKind(EntityKind),
    /// Two parameters of one action share a name.
    #[error("action {action} declares parameter {param} twice")]
    DuplicateParam {
        /// The action.
        action: ActionName,
        /// The parameter.
        param: ParamName,
    },
    /// An action names a kind no installed app declares. The registry raises it: it alone sees
    /// every installed manifest, so `validate` never does.
    #[error("action {action} names the unknown kind {kind}")]
    UnknownKind {
        /// The action.
        action: ActionName,
        /// The kind.
        kind: EntityKind,
    },
    /// A write with no undo that is not declared destructive.
    #[error("action {0} writes without undo and is not destructive")]
    WriteWithoutUndo(ActionName),
    /// A read that declares undo.
    #[error("action {0} only reads and declares undo")]
    ReadWithUndo(ActionName),
    /// A defaulted parameter whose default does not fit its type.
    #[error("action {action}: the default of {param} does not fit its type")]
    DefaultOutOfType {
        /// The action.
        action: ActionName,
        /// The parameter.
        param: ParamName,
    },
    /// An action whose name does not start with its app's own prefix.
    #[error("action {0} does not start with its app's prefix")]
    ActionOutsideApp(ActionName),
    /// An outbound action with no recipient or destination parameter.
    #[error("outbound action {0} declares no recipient or destination")]
    OutboundWithoutSink(ActionName),
    /// An indexed kind with no `<kind>.open`: a hit of it could not be opened from the launcher.
    #[error("indexed kind {0} declares no {0}.open")]
    OpenMissing(EntityKind),
    /// A `<kind>.open` that is not one read of one thing of the kind with nothing required.
    #[error("action {0} must read one thing of its kind, with no undo and no required parameter")]
    BadOpen(ActionName),
    /// A kind that names one of its relations twice.
    #[error("kind {kind} declares the relation {relation} twice")]
    DuplicateRelation {
        /// The kind.
        kind: EntityKind,
        /// The relation.
        relation: RelationName,
    },
    /// An action that takes the name of a kind's derived related action but is not it.
    #[error("action {0} is the name of a derived related action; do not declare it")]
    RelatedDeclared(ActionName),
}

/// The name of the action that opens a thing of `kind` (`mail.thread` opens by `mail.thread.open`).
/// Every indexed kind declares it, and the launcher performs it on a search hit.
pub fn open_name(kind: &EntityKind) -> Option<ActionName> {
    ActionName::parse(&format!("{}.open", kind.as_str())).ok()
}

impl Manifest {
    /// The action that opens a thing of `kind`, if this app declares one.
    pub fn open_of(&self, kind: &EntityKind) -> Option<&ActionDecl> {
        let name = open_name(kind)?;
        self.actions.iter().find(|a| a.name == name)
    }
}

/// The open actions: one read of one thing of the kind, nothing required but the target.
fn check_opens(manifest: &Manifest) -> Result<(), ManifestError> {
    for e in &manifest.entities {
        match (manifest.open_of(&e.kind), e.index) {
            (None, IndexPolicy::Indexed) => {
                return Err(ManifestError::OpenMissing(e.kind.clone()));
            }
            (Some(a), _) => {
                let shape = a.on == TargetKind::One(e.kind.clone())
                    && a.effect == Effect::Read
                    && a.undo == UndoSupport::NotUndoable
                    && a.params.iter().all(|p| p.need != ParamNeed::Required);
                if !shape {
                    return Err(ManifestError::BadOpen(a.name.clone()));
                }
            }
            (None, IndexPolicy::NotIndexed) => {}
        }
    }
    Ok(())
}

/// A manifest that passed [`validate`]. The only way to make one is to validate.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "Manifest", into = "Manifest")]
pub struct ValidManifest(Manifest);

impl ValidManifest {
    /// The manifest.
    pub fn manifest(&self) -> &Manifest {
        &self.0
    }
}

impl TryFrom<Manifest> for ValidManifest {
    type Error = ManifestError;
    fn try_from(manifest: Manifest) -> Result<Self, ManifestError> {
        validate(manifest)
    }
}

impl From<ValidManifest> for Manifest {
    fn from(valid: ValidManifest) -> Manifest {
        valid.0
    }
}

/// Whether `value` is a legal value of `ty`: what a default must satisfy.
pub fn fits(value: &Value, ty: &ParamType) -> bool {
    match (value, ty) {
        (Value::Text(t), ParamType::Text { max, lines }) => {
            t.chars().count() <= max.0 as usize
                && (*lines == crate::value::Lines::Many || !t.contains('\n'))
        }
        (Value::Integer(n), ParamType::Integer { min, max }) => (*min..=*max).contains(n),
        (Value::Decimal(d), ParamType::Decimal { scale }) => d.scale == *scale,
        (Value::Date(_), ParamType::Date)
        | (Value::DateTime(_), ParamType::DateTime)
        | (Value::Duration(_), ParamType::Duration)
        | (Value::File(_), ParamType::File)
        | (Value::Url(_), ParamType::Url) => true,
        (Value::Choice(id), ParamType::Choice(options)) => options.iter().any(|o| &o.id == id),
        (Value::Entity(e), ParamType::Entity(kind)) => &e.kind == kind,
        (Value::Entities(es), ParamType::Entities(kind)) => es.iter().all(|e| &e.kind == kind),
        (Value::Choice(_) | Value::Entity(_), ParamType::Dynamic(_)) => true,
        _ => false,
    }
}

fn check_action(prefix: &str, a: &ActionDecl) -> Result<(), ManifestError> {
    let own = a.name.as_str().split('.').next() == Some(prefix);
    if !own {
        return Err(ManifestError::ActionOutsideApp(a.name.clone()));
    }
    match (a.effect, a.undo) {
        (Effect::Read, UndoSupport::Token) => {
            return Err(ManifestError::ReadWithUndo(a.name.clone()));
        }
        (Effect::UndoableWrite, UndoSupport::NotUndoable) => {
            return Err(ManifestError::WriteWithoutUndo(a.name.clone()));
        }
        _ => {}
    }
    let sinks: BTreeSet<ArgSink> = a.params.iter().map(|p| p.sink).collect();
    let routed = sinks.contains(&ArgSink::Recipient) || sinks.contains(&ArgSink::Destination);
    if a.effect == Effect::Outbound && !routed {
        return Err(ManifestError::OutboundWithoutSink(a.name.clone()));
    }
    let mut seen = BTreeSet::new();
    for p in &a.params {
        if !seen.insert(&p.name) {
            return Err(ManifestError::DuplicateParam {
                action: a.name.clone(),
                param: p.name.clone(),
            });
        }
        if let ParamNeed::Defaulted(v) = &p.need
            && !fits(v, &p.ty)
        {
            return Err(ManifestError::DefaultOutOfType {
                action: a.name.clone(),
                param: p.name.clone(),
            });
        }
    }
    Ok(())
}

/// Checks a manifest. The rules: the vocabulary is not newer; names are unique; every action
/// carries its app's prefix; a read declares no undo; an undoable write declares a token (a
/// write without undo is declared destructive, and an outbound action may declare a token when
/// the app holds the send); an outbound action declares a recipient or destination parameter;
/// a defaulted value fits its type; every indexed kind declares `<kind>.open` (one read of one
/// thing of the kind, nothing required), and any `<kind>.open` has that shape. Kinds named by an action are checked by the registry.
pub fn validate(manifest: Manifest) -> Result<ValidManifest, ManifestError> {
    if manifest.vocab > IntentsVocab::CURRENT {
        return Err(ManifestError::VocabTooNew(manifest.vocab));
    }
    let mut kinds = BTreeSet::new();
    for e in &manifest.entities {
        if !kinds.insert(&e.kind) {
            return Err(ManifestError::DuplicateKind(e.kind.clone()));
        }
    }
    let prefix = action_prefix(&manifest.app);
    let mut names = BTreeSet::new();
    for a in &manifest.actions {
        if !names.insert(&a.name) {
            return Err(ManifestError::DuplicateAction(a.name.clone()));
        }
        check_action(&prefix, a)?;
    }
    check_opens(&manifest)?;
    let manifest = with_related(manifest, &prefix)?;
    Ok(ValidManifest(manifest))
}

/// The manifest with the derived related action of every kind that declares relations. The
/// action is the same whatever the app wrote, so a manifest that already holds it (one that was
/// validated, written out and read back) is taken as it is, and one that declares an action
/// of that name by hand is refused.
fn with_related(mut manifest: Manifest, prefix: &str) -> Result<Manifest, ManifestError> {
    let mut derived = Vec::new();
    for e in &manifest.entities {
        let mut seen = BTreeSet::new();
        if let Some(r) = e.relations.iter().find(|r| !seen.insert(&r.name)) {
            return Err(ManifestError::DuplicateRelation {
                kind: e.kind.clone(),
                relation: r.name.clone(),
            });
        }
        let Some(action) = e.related_action(&manifest.entities) else {
            continue;
        };
        match manifest.actions.iter().find(|a| a.name == action.name) {
            Some(held) if *held == action => {}
            Some(held) => return Err(ManifestError::RelatedDeclared(held.name.clone())),
            None => {
                check_action(prefix, &action)?;
                derived.push(action);
            }
        }
    }
    manifest.actions.extend(derived);
    Ok(manifest)
}
