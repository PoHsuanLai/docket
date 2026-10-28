//! Relations: the derived `<kind>.related` action and the mistakes a relation can make.

use crate::support::*;
use docket_manifest::{
    ActionDecl, AgentReach, Cardinality, EntityDecl, IndexPolicy, Manifest, ManifestError,
    ParamNeed, RELATION_ARG, RelationDecl, ResultShape, TargetKind, TitleTrust, UndoSupport,
    ValidManifest, validate,
};
use docket_vocab::{ParamType, RelationName};
use prov::Effect;

fn relation(name: &str, to: &str, trust: TitleTrust) -> RelationDecl {
    RelationDecl {
        name: RelationName::parse(name).expect("relation"),
        label: words("Sender"),
        to: kind(to),
        many: Cardinality::One,
        trust,
    }
}

fn contact_kind() -> EntityDecl {
    EntityDecl {
        kind: kind("mail.contact"),
        class: prov::DataClass::Contacts,
        ..thread_kind(IndexPolicy::NotIndexed)
    }
}

fn with_relations(relations: Vec<RelationDecl>, actions: Vec<ActionDecl>) -> Manifest {
    let mut thread = thread_kind(IndexPolicy::NotIndexed);
    thread.relations = relations;
    let mut m = manifest("org.quire.Mail", actions);
    m.entities = vec![thread, contact_kind()];
    m
}

#[test]
fn a_kind_with_relations_gets_a_derived_read_action_that_asks_for_the_classes_of_both_and_one_with_none_gets_none()
 {
    let from = relation("from", "mail.contact", TitleTrust::AppAuthored);
    let valid = validate(with_relations(vec![from], vec![])).expect("valid");
    let derived = valid
        .manifest()
        .actions
        .iter()
        .find(|a| a.name.as_str() == "mail.thread.related")
        .expect("derived action");
    assert_eq!(derived.on, TargetKind::One(kind("mail.thread")));
    assert_eq!(derived.effect, Effect::Read);
    assert_eq!(derived.undo, UndoSupport::NotUndoable);
    assert_eq!(derived.reach, AgentReach::Offered);
    assert_eq!(
        derived.classes,
        [prov::DataClass::Mail, prov::DataClass::Contacts]
            .into_iter()
            .collect()
    );
    assert_eq!(derived.result, ResultShape::Entities(kind("mail.contact")));
    let [relation] = derived.params.as_slice() else {
        panic!("one parameter: {:?}", derived.params)
    };
    assert_eq!(relation.name.as_str(), RELATION_ARG);
    assert_eq!(relation.need, ParamNeed::Required);
    assert!(matches!(&relation.ty, ParamType::Choice(options)
        if options.len() == 1 && options[0].id.as_str() == "from"));
    assert_eq!(
        valid
            .manifest()
            .related_kind(&derived.name)
            .map(|e| &e.kind),
        Some(&kind("mail.thread"))
    );
    // Step two: the same kind with no relation gets no derived action.
    let bare = validate(with_relations(vec![], vec![])).expect("valid");
    assert!(
        bare.manifest().actions.is_empty(),
        "step two: no relation, no derived action"
    );
}

#[test]
fn a_validated_manifest_written_and_read_back_is_the_same_manifest() {
    let from = relation("from", "mail.contact", TitleTrust::AppAuthored);
    let valid = validate(with_relations(vec![from], vec![])).expect("valid");
    let json = serde_json::to_string(&valid).expect("json");
    let back: ValidManifest = serde_json::from_str(&json).expect("valid manifest");
    assert_eq!(back, valid);
    assert_eq!(
        back.manifest().actions.len(),
        1,
        "the action is not added twice"
    );
}

#[test]
fn a_manifest_without_relations_does_not_write_the_field() {
    let json = serde_json::to_string(&thread_kind(IndexPolicy::NotIndexed)).expect("json");
    assert!(!json.contains("relations"), "{json}");
    let back: EntityDecl = serde_json::from_str(&json).expect("an older file parses");
    assert!(back.relations.is_empty());
}

#[test]
fn relation_mistakes_are_refused() {
    use ManifestError as E;
    let from = || relation("from", "mail.contact", TitleTrust::AppAuthored);
    let twice = with_relations(vec![from(), from()], vec![]);
    assert_eq!(
        validate(twice).err(),
        Some(E::DuplicateRelation {
            kind: kind("mail.thread"),
            relation: RelationName::parse("from").expect("relation"),
        })
    );
    let own = decl(
        "mail.thread.related",
        Effect::Read,
        UndoSupport::NotUndoable,
    );
    let declared = with_relations(vec![from()], vec![own]);
    assert_eq!(
        validate(declared).err(),
        Some(E::RelatedDeclared(action("mail.thread.related")))
    );
}
