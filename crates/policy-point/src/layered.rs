//! Building one policy set from layers. Failure never loosens: a bad person or assistant source
//! is left out and reported, a bad preset is replaced by the compiled-in standard preset, and a
//! bad core or administrator source is an error the caller must answer by asking for everything.

use crate::layers::{Layer, PolicyOrigin, PolicySource, Rejected};
use crate::pdp::{Pdp, PolicyError};
use cedar_policy::{Effect, PolicySet, Schema, ValidationMode, Validator};
use std::collections::BTreeMap;
use std::str::FromStr;

/// A policy set built from layers, with the sources it had to leave out.
#[derive(Debug)]
pub struct Layered {
    /// The merged, validated set.
    pub pdp: Pdp,
    /// The person's and assistant's sources that were left out, and a preset that was replaced.
    pub skipped: Vec<Rejected>,
}

/// Why no policy set could be built from the layers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LayerError {
    /// The schema does not parse.
    #[error("schema: {0}")]
    Schema(String),
    /// A core source is bad: the build itself is wrong.
    #[error("{0}")]
    Core(Rejected),
    /// The compiled-in preset is bad, so there is nothing to fall back to.
    #[error("{0}")]
    BuiltIn(Rejected),
    /// An administrator source is bad. It is never left out: the caller makes every agent call
    /// ask, or does not start.
    #[error("{0}")]
    Admin(Rejected),
    /// The merged set does not validate against the schema.
    #[error("merged set: {0}")]
    Merged(String),
}

/// The sources admitted so far.
#[derive(Clone)]
struct Book {
    schema: Schema,
    texts: Vec<String>,
    ids: BTreeMap<String, PolicyOrigin>,
}

impl Book {
    /// Admits `source` whole, or none of it. It must parse, validate strictly on its own, give
    /// no permit where only forbids are allowed, and reuse no `@id` already admitted.
    fn admit(&mut self, source: &PolicySource) -> Result<(), Rejected> {
        let refuse = |why: String| Rejected {
            layer: source.layer,
            origin: source.origin.clone(),
            why,
        };
        let text = source
            .text
            .as_ref()
            .map_err(|why| refuse(format!("cannot be read: {why}")))?;
        let set = PolicySet::from_str(text).map_err(|e| refuse(e.to_string()))?;
        if source.layer.forbids_only() && set.policies().any(|p| p.effect() == Effect::Permit) {
            return Err(refuse(format!(
                "the {} layer may only forbid, and this file has a permit",
                source.layer
            )));
        }
        let result = Validator::new(self.schema.clone()).validate(&set, ValidationMode::Strict);
        if !result.validation_passed() {
            let faults: Vec<String> = result.validation_errors().map(|e| e.to_string()).collect();
            return Err(refuse(faults.join("; ")));
        }
        let mut mine: BTreeMap<String, PolicyOrigin> = BTreeMap::new();
        for id in set.policies().filter_map(|p| p.annotation("id")) {
            let earlier = self.ids.get(id).or_else(|| mine.get(id));
            if let Some(earlier) = earlier {
                return Err(refuse(format!(
                    "the id \"{id}\" is also used by {earlier} and {}",
                    source.origin
                )));
            }
            mine.insert(id.to_owned(), source.origin.clone());
        }
        self.ids.extend(mine);
        self.texts.push(text.clone());
        Ok(())
    }

    fn admit_all<'a>(
        &mut self,
        sources: impl IntoIterator<Item = &'a PolicySource>,
    ) -> Result<(), Rejected> {
        sources.into_iter().try_for_each(|s| self.admit(s))
    }
}

impl Pdp {
    /// Builds one set from `sources`, in layer order (files of one layer in the order given),
    /// against the schema text `schema`. The built-in core and preset come from
    /// [`PolicySource::core`] and [`PolicySource::standard_preset`]; with no preset source the
    /// built-in preset stands, unreported.
    ///
    /// A source that is refused is handled by its layer: person and assistant sources are left
    /// out and listed in `skipped`; any bad preset source drops the whole preset for the built-in
    /// one (listed too); a bad core, built-in or administrator source is an error.
    pub fn layered(schema: &str, sources: &[PolicySource]) -> Result<Layered, LayerError> {
        let (schema, _warnings) =
            Schema::from_cedarschema_str(schema).map_err(|e| LayerError::Schema(e.to_string()))?;
        let in_layer = |layer: Layer| sources.iter().filter(move |s| s.layer == layer);
        let mut book = Book {
            schema,
            texts: Vec::new(),
            ids: BTreeMap::new(),
        };
        let mut skipped = Vec::new();
        book.admit_all(in_layer(Layer::Core))
            .map_err(LayerError::Core)?;
        let mut trial = book.clone();
        match trial.admit_all(in_layer(Layer::Preset)) {
            Ok(()) if trial.texts.len() > book.texts.len() => {
                book = trial;
            }
            Ok(()) => book
                .admit(&PolicySource::standard_preset())
                .map_err(LayerError::BuiltIn)?,
            Err(rejected) => {
                skipped.push(rejected);
                book.admit(&PolicySource::standard_preset())
                    .map_err(LayerError::BuiltIn)?;
            }
        }
        for source in in_layer(Layer::Person).chain(in_layer(Layer::Assistant)) {
            if let Err(rejected) = book.admit(source) {
                skipped.push(rejected);
            }
        }
        book.admit_all(in_layer(Layer::Admin))
            .map_err(LayerError::Admin)?;
        let policies = PolicySet::from_str(&book.texts.join("\n"))
            .map_err(|e| LayerError::Merged(e.to_string()))?;
        let pdp = Pdp::checked(policies, book.schema).map_err(|e| match e {
            PolicyError::Schema(why) | PolicyError::Parse(why) | PolicyError::Invalid(why) => {
                LayerError::Merged(why)
            }
        })?;
        Ok(Layered { pdp, skipped })
    }
}
