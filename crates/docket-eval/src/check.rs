//! `docket-eval --check-app <dir>`: "every app has a CLI" holds by construction (cli.md section 6).
//! The check reads an app's repository as data and fails when
//!
//! - the app ships a `.desktop` file and no intents manifest (a reverse-DNS desktop file names
//!   its app, which then needs that app's manifest; any other needs at least one);
//! - a manifest does not validate (`ValidManifest`), which includes an action with no `params`
//!   key: the parameter schema is required, written even when empty;
//! - a menu command or keyboard shortcut declared in `<AppName>.ui.toml` names no action of the
//!   manifest and is not listed under `ui_only` with a reason (`ui`).
//!
//! The third rule needs data no app emits yet (ds menu items carry no action id); the file shape
//! is the interface ask in `FINDINGS.md`, and until an app ships one the check says so and does
//! not fail it.
//!
//! A manifest is `*.toml` under a directory named `intents` (what is installed under
//! `$XDG_DATA_DIRS/quire/intents/`) or `*.intents.toml`; test fixtures (`tests`, `fixtures`,
//! `testdata`), build output and hidden directories are not part of the app.

use crate::ui::{UiFile, check_ui};
use docket_core::{ValidManifest, tool_schema};
use docket_router::parse;
use porter_core::AppName;
use std::path::{Path, PathBuf};

/// How serious a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Level {
    /// Something that holds.
    Pass,
    /// Something to know, not a failure.
    Note,
    /// The app breaks the rule.
    Fail,
}

/// One line of the report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// How serious.
    pub level: Level,
    /// The file it is about, relative to the repository.
    pub file: PathBuf,
    /// What it says.
    pub what: String,
}

/// Everything the check found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CheckReport {
    /// In the order found.
    pub findings: Vec<Finding>,
}

impl CheckReport {
    /// How many findings are failures.
    pub fn failures(&self) -> usize {
        self.findings
            .iter()
            .filter(|f| f.level == Level::Fail)
            .count()
    }

    /// The report as lines: `ok`, `note` or `FAIL`, the file and the words.
    pub fn render(&self) -> String {
        self.findings
            .iter()
            .map(|f| {
                let word = match f.level {
                    Level::Pass => "ok  ",
                    Level::Note => "note",
                    Level::Fail => "FAIL",
                };
                format!("{word} {}: {}\n", f.file.display(), f.what)
            })
            .collect()
    }

    pub(crate) fn push(&mut self, level: Level, file: &Path, what: impl Into<String>) {
        self.findings.push(Finding {
            level,
            file: file.to_owned(),
            what: what.into(),
        });
    }
}

/// The directory could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{path}: {why}")]
pub struct CheckError {
    /// Where.
    pub path: PathBuf,
    /// Why.
    pub why: String,
}

/// What a walk of the repository found.
#[derive(Debug, Default)]
pub(crate) struct Found {
    pub(crate) desktops: Vec<PathBuf>,
    pub(crate) manifests: Vec<PathBuf>,
    pub(crate) ui: Vec<PathBuf>,
    /// Directories that hold a `skill.toml`, relative to the root.
    pub(crate) skills: Vec<PathBuf>,
}

fn skipped(name: &str) -> bool {
    name.starts_with('.')
        || matches!(
            name,
            "target" | "node_modules" | "tests" | "fixtures" | "testdata"
        )
}

pub(crate) fn walk(root: &Path, dir: &Path, found: &mut Found) -> Result<(), CheckError> {
    let io = |e: std::io::Error| CheckError {
        path: dir.to_owned(),
        why: e.to_string(),
    };
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(io)?
        .filter_map(Result::ok)
        .collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    if dir.join("skill.toml").is_file() {
        found
            .skills
            .push(dir.strip_prefix(root).unwrap_or(dir).to_owned());
    }
    for entry in entries {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !skipped(&name) {
                walk(root, &path, found)?;
            }
            continue;
        }
        let relative = path.strip_prefix(root).unwrap_or(&path).to_owned();
        let in_intents = path
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|d| d == "intents");
        if name.ends_with(".ui.toml") {
            found.ui.push(relative);
        } else if name.ends_with(".desktop") || name.ends_with(".desktop.in") {
            found.desktops.push(relative);
        } else if name.ends_with(".intents.toml") || (in_intents && name.ends_with(".toml")) {
            found.manifests.push(relative);
        }
    }
    Ok(())
}

/// Whether a desktop entry is an application (a link or a directory entry is not).
fn is_application(text: &str) -> bool {
    let kind = text
        .lines()
        .find_map(|l| l.strip_prefix("Type="))
        .map(str::trim);
    kind.is_none() || kind == Some("Application")
}

/// The app a desktop file names, when its stem is a reverse-DNS app name.
fn app_of(desktop: &Path) -> Option<AppName> {
    let name = desktop.file_name()?.to_string_lossy();
    let stem = name
        .strip_suffix(".desktop.in")
        .or_else(|| name.strip_suffix(".desktop"))?;
    AppName::parse(stem).ok()
}

pub(crate) fn read(root: &Path, relative: &Path) -> Result<String, CheckError> {
    std::fs::read_to_string(root.join(relative)).map_err(|e| CheckError {
        path: relative.to_owned(),
        why: e.to_string(),
    })
}

/// Checks the app whose repository is `root`.
pub fn check_app(root: &Path) -> Result<CheckReport, CheckError> {
    let mut found = Found::default();
    walk(root, root, &mut found)?;
    let mut report = CheckReport::default();

    let mut valid: Vec<(PathBuf, ValidManifest)> = Vec::new();
    for file in &found.manifests {
        match parse(&read(root, file)?) {
            Ok(manifest) => {
                let actions = manifest.manifest().actions.len();
                let schemas = manifest
                    .manifest()
                    .actions
                    .iter()
                    .all(|a| tool_schema(a).0.is_object());
                if schemas {
                    report.push(
                        Level::Pass,
                        file,
                        format!(
                            "{} validates: {actions} actions, each with its parameter schema",
                            manifest.manifest().app
                        ),
                    );
                    valid.push((file.clone(), manifest));
                } else {
                    report.push(Level::Fail, file, "an action has no parameter schema");
                }
            }
            Err(why) => report.push(Level::Fail, file, format!("not a valid manifest: {why}")),
        }
    }

    for desktop in &found.desktops {
        if !is_application(&read(root, desktop)?) {
            continue;
        }
        let covered = match app_of(desktop) {
            Some(app) => valid.iter().any(|(_, m)| m.manifest().app == app),
            None => !valid.is_empty(),
        };
        if covered {
            report.push(Level::Pass, desktop, "the app ships an intents manifest");
        } else {
            let wants = app_of(desktop).map_or_else(
                || "intents manifest".to_owned(),
                |app| format!("intents manifest for {app}"),
            );
            report.push(
                Level::Fail,
                desktop,
                format!("the app ships a .desktop file and no {wants}: every app we ship is drivable from a command line, generated from its manifest"),
            );
        }
    }

    for (file, manifest) in &valid {
        let app = &manifest.manifest().app;
        let ui = found.ui.iter().find(|f| {
            f.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with(app.as_str()))
        });
        match ui {
            Some(ui_file) => match UiFile::parse(&read(root, ui_file)?) {
                Ok(parsed) => check_ui(&mut report, ui_file, manifest, &parsed),
                Err(why) => report.push(Level::Fail, ui_file, format!("not a ui commands file: {why}")),
            },
            None => report.push(
                Level::Note,
                file,
                format!(
                    "no {app}.ui.toml: the menu commands and shortcuts are not checked (ds menu items carry no action id yet; see FINDINGS.md)"
                ),
            ),
        }
    }
    Ok(report)
}
