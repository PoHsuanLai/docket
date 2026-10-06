//! `quire-do skills`: the installed skills, from the directories `main` read, each checked against
//! the manifests intentd has registered: offered to the companion, hidden for a missing action,
//! or not offered because an action is Hidden. A directory that does not load is listed with why.

use crate::exec::Printed;
use crate::resolve::Apps;
use docket_core::IntentsVocab;
use docket_skills::{Origin, Reach, Roots, Skill, discover, reach};
use serde_json::{Value, json};

fn origin(skill: &Skill) -> &'static str {
    match skill.origin {
        Origin::Shipped => "shipped",
        Origin::Own => "own",
    }
}

fn state(skill: &Skill, apps: &Apps) -> (&'static str, Option<String>) {
    match reach(skill, apps.all()) {
        Reach::Reachable => ("offered", None),
        Reach::Missing(a) => ("hidden", Some(format!("{}:{} is not registered", a.app, a.name))),
        Reach::Hidden(a) => (
            "not offered",
            Some(format!("{}:{} is hidden from the companion", a.app, a.name)),
        ),
    }
}

/// The skills under `roots`, as lines and as JSON.
pub fn list(apps: &Apps, roots: &Roots) -> Printed {
    let found = discover(roots);
    let mut lines = Vec::new();
    let mut rows: Vec<Value> = Vec::new();
    for skill in &found.skills {
        let (word, why) = state(skill, apps);
        let tail = why.as_ref().map(|w| format!(" ({w})")).unwrap_or_default();
        lines.push(format!(
            "{:<20} {:<8} {:<11} {}  [{}, {}]{tail}",
            skill.id,
            skill.version.0,
            word,
            skill.description,
            skill.owner,
            origin(skill)
        ));
        rows.push(json!({
            "id": skill.id,
            "version": skill.version,
            "owner": skill.owner,
            "origin": origin(skill),
            "state": word,
            "why": why,
            "description": skill.description,
            "uses": skill.uses.iter().map(|u| format!("{}:{}", u.app, u.name)).collect::<Vec<_>>(),
        }));
    }
    let broken: Vec<Value> = found
        .rejected
        .iter()
        .map(|r| {
            lines.push(format!("{}: not loaded: {}", r.dir.display(), r.fault));
            json!({ "dir": r.dir.display().to_string(), "fault": r.fault.to_string() })
        })
        .collect();
    if lines.is_empty() {
        lines.push("no skills installed".to_owned());
    }
    Printed {
        human: lines.join("\n"),
        json: json!({ "vocab": IntentsVocab::CURRENT, "skills": rows, "rejected": broken }),
    }
}
