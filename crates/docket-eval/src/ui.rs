//! The app's menu commands and keyboard shortcuts, as data, for rule three of the conformance
//! check: each is the face of a declared action, or it is listed as UI-only with a reason
//! (window chrome, scrolling, and so on). The file is `<AppName>.ui.toml`, written by the app's
//! build from its ds menu bar and its shortcut table:
//!
//! ```toml
//! vocab = 1
//! app = "org.quire.Mail"
//!
//! [[commands]]            # one per menu item and one per shortcut binding
//! id = "file.new"         # the app's stable id for it
//! source = "menu"         # menu | shortcut
//! chord = "cmd+n"         # optional: the key equivalent shown or bound
//! action = "mail.draft.create"
//!
//! [[ui_only]]             # the ones that are not an action
//! id = "window.minimize"
//! reason = "window chrome"
//! ```

use crate::check::{CheckReport, Level};
use docket_core::ValidManifest;
use prov::ActionName;
use serde::Deserialize;
use std::path::Path;

/// Where a command is declared.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UiSource {
    /// A ds menu item.
    Menu,
    /// A keyboard shortcut binding.
    Shortcut,
}

/// One menu item or shortcut.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UiCommand {
    /// The app's stable id for it.
    pub id: String,
    /// Menu or shortcut.
    pub source: UiSource,
    /// The key equivalent, if any.
    #[serde(default)]
    pub chord: Option<String>,
    /// The action it is the face of.
    #[serde(default)]
    pub action: Option<ActionName>,
}

/// A command that is not an action, and why.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UiOnly {
    /// The command's id.
    pub id: String,
    /// Why it has no action.
    pub reason: String,
}

/// The file.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct UiFile {
    /// The vocabulary (the intents vocabulary of the manifest).
    pub vocab: u32,
    /// The app.
    pub app: porter_core::AppName,
    /// The commands.
    pub commands: Vec<UiCommand>,
    /// The ones with no action.
    #[serde(default)]
    pub ui_only: Vec<UiOnly>,
}

/// Why a ui commands file could not be read.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct UiFault(#[from] toml::de::Error);

impl UiFile {
    /// Reads the file.
    pub fn parse(text: &str) -> Result<Self, UiFault> {
        Ok(toml::from_str(text)?)
    }
}

/// Rule three: every command names a declared action or is listed UI-only with a reason.
pub fn check_ui(report: &mut CheckReport, file: &Path, manifest: &ValidManifest, ui: &UiFile) {
    let declared = &manifest.manifest().actions;
    if ui.app != manifest.manifest().app {
        report.push(
            Level::Fail,
            file,
            format!(
                "this file is for {}, not {}",
                ui.app,
                manifest.manifest().app
            ),
        );
        return;
    }
    let mut failed = 0;
    for command in &ui.commands {
        let only = ui.ui_only.iter().find(|o| o.id == command.id);
        let what = match (&command.action, only) {
            (Some(action), _) if declared.iter().any(|a| &a.name == action) => continue,
            (Some(action), _) => format!(
                "{:?} {} names {action}, which the manifest does not declare",
                command.source, command.id
            ),
            (None, Some(o)) if !o.reason.trim().is_empty() => continue,
            (None, Some(_)) => format!(
                "{:?} {} is listed under ui_only with no reason",
                command.source, command.id
            ),
            (None, None) => format!(
                "{:?} {} has no action and is not listed under ui_only",
                command.source, command.id
            ),
        };
        failed += 1;
        report.push(Level::Fail, file, what);
    }
    if failed == 0 {
        report.push(
            Level::Pass,
            file,
            format!(
                "{} menu commands and shortcuts, each an action or UI-only with a reason",
                ui.commands.len()
            ),
        );
    }
}
