//! What a helper may reach, from what the person said: the apps it may use and how far it may go
//! in them, as the task policy every job of it is cut to. The person never writes a policy; they
//! name apps and choose a ceiling, and the policy covers those apps' things up to it, trusting no
//! recipient, destination or path (anything a job sends out or writes to a place still asks).

use docket_manifest::Manifest;
use docket_permissions_core::{ActionMatch, TaskPolicy, TaskPolicyState};
use docket_vocab::LabelText;
use porter_core::AppName;
use porter_ids::Count;
use prov::{Effect, SpaceId, TaskId, UnixSeconds};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

/// How far a helper may go in the apps it may use.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Ceiling {
    /// Look at things.
    Look,
    /// Look at things and change them, in ways that can be undone.
    Change,
    /// Also send things out.
    Send,
}

impl Ceiling {
    /// The most consequential effect it allows.
    pub fn effect(self) -> Effect {
        match self {
            Ceiling::Look => Effect::Read,
            Ceiling::Change => Effect::UndoableWrite,
            Ceiling::Send => Effect::Outbound,
        }
    }
}

/// Why what the person named is not a reach.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum ReachFault {
    /// No app was named.
    #[error("name at least one app for it to use")]
    NoApps,
    /// An app that is not installed.
    #[error("there is no app called {0}")]
    NoSuchApp(String),
}

/// The most things one call of a helper may touch.
const MOST_AT_ONCE: Count = Count(20);

/// The reach for `apps` up to `ceiling` in `space`, over the installed `manifests`: the policy's
/// kinds are the kinds of thing the named apps own. It never expires and trusts no sink.
pub fn reach_policy(
    apps: &[AppName],
    ceiling: Ceiling,
    space: &SpaceId,
    installed: &[&Manifest],
) -> Result<TaskPolicy, ReachFault> {
    if apps.is_empty() {
        return Err(ReachFault::NoApps);
    }
    let effect = ceiling.effect();
    let mut actions = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    for app in apps {
        let manifest = installed
            .iter()
            .find(|m| &m.app == app)
            .ok_or_else(|| ReachFault::NoSuchApp(app.to_string()))?;
        actions.insert(ActionMatch::AppUpTo(app.clone(), effect));
        kinds.extend(manifest.entities.iter().map(|e| e.kind.clone()));
    }
    let (Ok(task), Ok(rationale)) = (
        TaskId::parse("t-0"),
        LabelText::parse("what the helper may use"),
    ) else {
        return Err(ReachFault::NoApps);
    };
    Ok(TaskPolicy {
        task,
        space: space.clone(),
        from: Vec::new(),
        actions,
        kinds,
        ceiling: effect,
        max_count: MOST_AT_ONCE,
        recipients: Vec::new(),
        destinations: Vec::new(),
        paths: Vec::new(),
        expires: UnixSeconds(i64::MAX),
        rationale,
        state: TaskPolicyState::Active,
    })
}

fn how_far(effect: Effect) -> &'static str {
    match effect {
        Effect::Read => "look at things",
        Effect::UndoableWrite => "look at things and change them (changes can be undone)",
        Effect::Outbound => "look at things, change them and send things out",
        _ => "do anything, including deleting",
    }
}

/// What a reach lets a helper do, one plain line per app: "org.quire.Mail: look at things".
pub fn reach_lines(policy: &TaskPolicy) -> Vec<String> {
    policy
        .actions
        .iter()
        .map(|m| match m {
            ActionMatch::One(action) => format!("{}: {}", action.app, action.name),
            ActionMatch::AppUpTo(app, effect) => format!("{app}: {}", how_far(*effect)),
        })
        .collect()
}
