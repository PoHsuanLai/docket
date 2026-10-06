//! `docket-eval --check-skills <dir>…`: every skill under the directories validates (the format,
//! the sizes, the ids agreeing, vocab 1) and every action it `uses` exists in a manifest found
//! under the same directories or among the built-in ones. A skill whose action is missing would
//! be hidden at run time, so here it fails. A skill that uses an action the companion cannot
//! reach (Hidden) is a note: it is valid and simply not offered.
//!
//! Skills are the directories that hold a `skill.toml`; manifests are found as `--check-app`
//! finds them. Name every repo whose actions a skill uses, so each `uses` can be resolved.

use crate::check::{CheckError, CheckReport, Found, Level, read, walk};
use docket_core::ValidManifest;
use docket_router::parse;
use docket_skills::{Origin, Reach, SkillFault, load_dir, reach};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const BUILT_IN: [&str; 2] = [
    include_str!("../../../manifests/org.quire.Memory.toml"),
    include_str!("../../../manifests/org.quire.Companion.toml"),
];

fn manifests_of(root: &Path, found: &Found, report: &mut CheckReport) -> Vec<ValidManifest> {
    let mut manifests = Vec::new();
    for file in &found.manifests {
        match read(root, file).map(|t| parse(&t)) {
            Ok(Ok(valid)) => manifests.push(valid),
            Ok(Err(why)) => report.push(Level::Note, file, format!("manifest skipped: {why}")),
            Err(why) => report.push(Level::Note, file, format!("manifest skipped: {}", why.why)),
        }
    }
    manifests
}

/// Checks the skills and manifests under `dirs`.
pub fn check_skills(dirs: &[PathBuf]) -> Result<CheckReport, CheckError> {
    let mut report = CheckReport::default();
    let mut manifests: Vec<ValidManifest> = BUILT_IN.iter().filter_map(|t| parse(t).ok()).collect();
    let mut skill_dirs: Vec<PathBuf> = Vec::new();
    for root in dirs {
        let mut found = Found::default();
        walk(root, root, &mut found)?;
        manifests.extend(manifests_of(root, &found, &mut report));
        skill_dirs.extend(found.skills.iter().map(|d| root.join(d)));
    }
    let mut ids = BTreeSet::new();
    for dir in &skill_dirs {
        match load_dir(dir, Origin::Shipped) {
            Err(fault) => report.push(Level::Fail, dir, fault_words(&fault)),
            Ok(skill) if !ids.insert(skill.id.clone()) => {
                report.push(Level::Fail, dir, format!("the id {} is shipped twice", skill.id));
            }
            Ok(skill) => match reach(&skill, &manifests) {
                Reach::Reachable => report.push(
                    Level::Pass,
                    dir,
                    format!(
                        "{} {} validates: {} bytes, {} action(s) used, all registered",
                        skill.id,
                        skill.version.0,
                        skill.body.len(),
                        skill.uses.len()
                    ),
                ),
                Reach::Missing(action) => report.push(
                    Level::Fail,
                    dir,
                    format!(
                        "{} uses {}:{}, which no manifest declares: the skill would be hidden",
                        skill.id, action.app, action.name
                    ),
                ),
                Reach::Hidden(action) => report.push(
                    Level::Note,
                    dir,
                    format!(
                        "{} uses {}:{}, which is Hidden from the companion: it is not offered",
                        skill.id, action.app, action.name
                    ),
                ),
            },
        }
    }
    if skill_dirs.is_empty() {
        report.push(Level::Note, Path::new("."), "no skill.toml found");
    }
    Ok(report)
}

fn fault_words(fault: &SkillFault) -> String {
    format!("not a valid skill: {fault}")
}
