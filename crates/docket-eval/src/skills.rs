//! `docket-eval --check-skills <dir>…`: every skill under the directories validates (the format,
//! the sizes, the ids agreeing, vocab 1) and every action it `uses` exists in a manifest found
//! under the same directories or among the built-in ones. A skill whose action is missing would
//! be hidden at run time, so here it fails. A skill that uses an action the companion cannot
//! reach (Hidden) is a note: it is valid and simply not offered.
//!
//! Skills are the directories that hold a `skill.toml`; manifests are found as `--check-app`
//! finds them, and also where apps keep them: `dist/intents/*.toml` of each given root, and
//! `../intents` of a root named `skills` (so `<repo>/dist/skills` finds `<repo>/dist/intents`).
//! `--manifests <dir>` adds a directory of manifests (its `*.toml` files). Name every repo whose
//! actions a skill uses, so each `uses` can be resolved.

use crate::check::{CheckError, CheckReport, Found, Level, walk};
use docket_core::ValidManifest;
use docket_router::parse;
use docket_skills::{Origin, Reach, SkillFault, load_dir, reach};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const BUILT_IN: [&str; 2] = [
    include_str!("../../../manifests/org.quire.Memory.toml"),
    include_str!("../../../manifests/org.quire.Companion.toml"),
];

/// The `*.toml` files directly in `dir` (none when it is not a directory), sorted.
fn toml_files(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "toml"))
        .collect();
    files.sort();
    files
}

/// Where an app keeps its manifests next to a root: `dist/intents` under it, and, for a root
/// that is a repository's `skills` directory, the `intents` directory beside it.
fn app_manifest_dirs(root: &Path) -> Vec<PathBuf> {
    let beside = (root.file_name().is_some_and(|n| n == "skills"))
        .then(|| root.parent().map(|p| p.join("intents")))
        .flatten();
    std::iter::once(root.join("dist").join("intents"))
        .chain(beside)
        .collect()
}

/// Reads each manifest file once, whichever way it was found.
fn read_manifests(
    files: impl IntoIterator<Item = PathBuf>,
    seen: &mut BTreeSet<PathBuf>,
    report: &mut CheckReport,
) -> Vec<ValidManifest> {
    let mut manifests = Vec::new();
    for file in files {
        let same = std::fs::canonicalize(&file).unwrap_or_else(|_| file.clone());
        if !seen.insert(same) {
            continue;
        }
        match std::fs::read_to_string(&file).map(|t| parse(&t)) {
            Ok(Ok(valid)) => manifests.push(valid),
            Ok(Err(why)) => report.push(Level::Note, &file, format!("manifest skipped: {why}")),
            Err(why) => report.push(Level::Note, &file, format!("manifest skipped: {why}")),
        }
    }
    manifests
}

/// Checks the skills and manifests under `dirs`, with the manifests of `manifest_dirs` as well.
pub fn check_skills(
    dirs: &[PathBuf],
    manifest_dirs: &[PathBuf],
) -> Result<CheckReport, CheckError> {
    let mut report = CheckReport::default();
    let mut manifests: Vec<ValidManifest> = BUILT_IN.iter().filter_map(|t| parse(t).ok()).collect();
    let mut skill_dirs: Vec<PathBuf> = Vec::new();
    let mut seen = BTreeSet::new();
    for root in dirs {
        let mut found = Found::default();
        walk(root, root, &mut found)?;
        let walked = found.manifests.iter().map(|f| root.join(f));
        let kept = app_manifest_dirs(root)
            .into_iter()
            .flat_map(|d| toml_files(&d));
        manifests.extend(read_manifests(walked.chain(kept), &mut seen, &mut report));
        skill_dirs.extend(found.skills.iter().map(|d| root.join(d)));
    }
    let extra = manifest_dirs.iter().flat_map(|d| toml_files(d));
    manifests.extend(read_manifests(extra, &mut seen, &mut report));
    let mut ids = BTreeSet::new();
    for dir in &skill_dirs {
        match load_dir(dir, Origin::Shipped) {
            Err(fault) => report.push(Level::Fail, dir, fault_words(&fault)),
            Ok(skill) if !ids.insert(skill.id.clone()) => {
                report.push(
                    Level::Fail,
                    dir,
                    format!("the id {} is shipped twice", skill.id),
                );
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
