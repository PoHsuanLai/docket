//! Manifest validation: pure, total, one error per rule a caller can act on.

use crate::ids::{IntentsVocab, ParamName, action_prefix};
use crate::manifest::{ActionDecl, ArgSink, Manifest, ParamNeed, UndoSupport};
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
/// a defaulted value fits its type. Kinds named by an action are checked by the registry.
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
    Ok(ValidManifest(manifest))
}
