//! The arguments of a tool call the model made, read by the types the action declares. The model
//! speaks JSON; the router wants `Args` and a target. Every value is labelled as the planner's
//! own (the router derives the real labels itself and trusts none of them), and anything that
//! does not fit its declared type is a fault before the router is asked.

use crate::catalogue::CatalogueTool;
use docket_core::{Args, TargetValue, args_from_json};
use prov::{Integrity, Label, ModelRole, Source};
use serde_json::Value as Json;
use std::collections::BTreeSet;

pub use docket_core::ArgsFault;

/// What a tool call asks of the router, short of the origin.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadCall {
    /// What it acts on.
    pub target: TargetValue,
    /// Its arguments.
    pub args: Args,
}

/// The label of what a model wrote: its own words, untrusted, from the planner.
pub fn planner_label() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: prov::Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Model(ModelRole::Planner)]),
    }
}

/// Reads one tool call's JSON arguments, every value labelled as the planner's own.
pub fn read_call(tool: &CatalogueTool, json: &Json) -> Result<ReadCall, ArgsFault> {
    let object = json.as_object().ok_or(ArgsFault::NotAnObject)?;
    let (target, args) = args_from_json(&tool.decl, Some(object), &planner_label())?;
    Ok(ReadCall { target, args })
}
