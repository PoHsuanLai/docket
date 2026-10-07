//! A planner over a scripted model, and the view and catalogue the planner tests read it with.
#![allow(dead_code)]

use super::infer::{Say, ScriptedInfer};
use agent_loop::{Sources, assemble};
use companiond::*;
use docket_core::*;
use porter_core::{AppName, Count};
use prov::Integrity;
use std::path::PathBuf;

pub fn manifest(file: &str) -> ValidManifest {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../manifests")
        .join(file);
    let text = std::fs::read_to_string(&path).expect("manifest");
    docket_router::parse(&text).expect("valid")
}

pub fn catalogue() -> Catalogue {
    Catalogue::from_manifests(&[
        docket_fake::mail_manifest().expect("mail"),
        manifest("org.quire.Memory.toml"),
        manifest("org.quire.Companion.toml"),
    ])
}

pub fn view(cards: Vec<ActionCard>) -> PlannerView {
    let config = AgentConfig::default();
    assemble(
        &config.assembler,
        &Sources {
            cards,
            profile: vec![ProfileLine("Prefers short answers".into())],
            primer: Some(PrimerText("Eve is the landlord".into())),
            rollup: None,
            roster: Roster::default(),
            episodes: vec![],
            recalled: vec![],
            context: ContextView {
                app: AppName::parse("org.quire.Shell").expect("app"),
                window: Reveal::Plain(String::new()),
                here: HereView::Nowhere,
                selection: SelectionView::Nothing,
                visible: VisibleView {
                    kind: None,
                    items: vec![],
                    total: Count(0),
                },
                text_target: TextTargetView::None,
            },
            turns: vec![],
            history: vec![],
            handles: vec![],
            inbox: vec![],
            skills: vec![],
            skill_texts: vec![],
            taint: Integrity::Trusted,
            task_policy: None,
        },
    )
}

pub fn planner(script: Vec<Say>) -> (PlannerModel<ScriptedInfer>, ScriptedInfer) {
    let infer = ScriptedInfer::new(script);
    let c = catalogue();
    (PlannerModel::new(infer.clone()).with_catalogue(c), infer)
}
