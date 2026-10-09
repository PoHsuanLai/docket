//! What the router makes of an app's answer to a kind's related action (`<kind>.related`): the
//! things must be of the kind the relation names, as many as it says, and their label is never
//! better than the relation's declared trust or the label of the thing they were found from. The
//! app labels what it returns, but a relation whose answer comes from content somebody else wrote
//! (the sender of a message) is untrusted whatever the app says, so a recipient chosen through it
//! meets the same sink checks as any untrusted recipient.

use crate::prepared::Prepared;
use crate::router::Router;
use crate::seams::Seams;
use docket_core::{
    AppRefusal, Cardinality, EntityDecl, Manifest, Outcome, RELATION_ARG, RelationDecl, TitleTrust,
    Value,
};
use porter_core::DataClass;
use prov::{EntityKind, Label, Labelled};

/// The kind a related action resolves for and the relation it was asked for, if `p` is one.
fn asked<'a>(manifest: &'a Manifest, p: &Prepared) -> Option<(&'a EntityDecl, &'a RelationDecl)> {
    let kind = manifest.related_kind(&p.decl.name)?;
    let chosen = p.request.args.iter().find_map(|(name, arg)| {
        (name.as_str() == RELATION_ARG)
            .then_some(&arg.value)
            .and_then(|v| match v {
                Value::Choice(id) => Some(id),
                _ => None,
            })
    })?;
    let relation = kind
        .relations
        .iter()
        .find(|r| r.name.as_str() == chosen.as_str())?;
    Some((kind, relation))
}

/// Whether `value` names things of `kind`, no more than `many` allows.
fn conforms(value: &Option<Labelled<Value>>, kind: &EntityKind, many: Cardinality) -> bool {
    let things = match value.as_ref().map(|v| &v.value) {
        None => return true,
        Some(Value::Entity(e)) => std::slice::from_ref(e),
        Some(Value::Entities(es)) => es.as_slice(),
        Some(_) => return false,
    };
    things.iter().all(|e| &e.kind == kind) && (many == Cardinality::Many || things.len() <= 1)
}

impl<S: Seams> Router<S> {
    /// `outcome` as the router accepts it for `p`: unchanged for any action but a related one;
    /// for that, refused if it does not name things of the relation's kind, and labelled with the
    /// join of the app's label, the relation's declared trust and the label of every handle the
    /// call was made from.
    pub(crate) fn related_outcome(
        &self,
        p: &Prepared,
        mut outcome: Outcome,
    ) -> Result<Outcome, AppRefusal> {
        let st = self.locked();
        let Some(manifest) = st.registry.get(&p.request.action.app).map(|m| m.manifest()) else {
            return Ok(outcome);
        };
        let Some((_, relation)) = asked(manifest, p) else {
            return Ok(outcome);
        };
        if !conforms(&outcome.value, &relation.to, relation.many) {
            return Err(AppRefusal::Unsupported);
        }
        let class = st
            .registry
            .all()
            .flat_map(|m| &m.manifest().entities)
            .find(|e| e.kind == relation.to)
            .map_or(DataClass::AppOwn, |e| e.class);
        let floor = match &relation.trust {
            TitleTrust::AppAuthored => None,
            TitleTrust::ThirdParty(source) => {
                Some(Label::untrusted(source.clone(), class, p.space.clone()))
            }
        };
        let held = p
            .who
            .session
            .as_ref()
            .and_then(|s| st.sessions.get(s))
            .into_iter()
            .flat_map(|record| p.named.iter().filter_map(|h| record.handles.label(*h)))
            .cloned();
        if let Some(value) = outcome.value.as_mut() {
            value.label = floor
                .into_iter()
                .chain(held)
                .fold(value.label.clone(), |all, label| all.join(&label));
        }
        Ok(outcome)
    }
}
