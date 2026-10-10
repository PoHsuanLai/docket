//! The lines of the restore sheet: what the plan would do, in the words of the sheet. The paths
//! are named by agents and by the person, so each is drawn as quoted third-party text; a group
//! shows at most `SHOWN` of them and then how many more. Nothing here names a tool.

use crate::labels::{app_label, path_label};
use docket_core::{FactLine, LabelText, RestorePlan, WorkPath};
use porter_core::AppName;
use prov::{Label, Labelled};

/// How many paths of one group the sheet lists.
const SHOWN: usize = 20;

fn line(title: &str, value: String, label: Label) -> Option<FactLine> {
    Some(FactLine {
        label: LabelText::parse(title).ok()?,
        value: Labelled { value, label },
    })
}

fn files(count: usize) -> String {
    match count {
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    }
}

fn group(title: &str, paths: &[WorkPath], app: &Label) -> Vec<Option<FactLine>> {
    if paths.is_empty() {
        return Vec::new();
    }
    let mut lines = vec![line(title, files(paths.len()), app.clone())];
    lines.extend(
        paths
            .iter()
            .take(SHOWN)
            .map(|path| line("File", path.as_str().to_owned(), path_label())),
    );
    if paths.len() > SHOWN {
        let more = format!("and {} more", paths.len() - SHOWN);
        lines.push(line("More", more, app.clone()));
    }
    lines
}

/// The sheet's lines for restoring `plan`, as the app `app` (the provider) says them.
pub fn restore_sheet(plan: &RestorePlan, app: &AppName) -> Vec<FactLine> {
    let said = app_label(app);
    let mut lines = Vec::new();
    lines.extend(group("Put back older versions", &plan.changed, &said));
    lines.extend(group("Bring back deleted files", &plan.added, &said));
    lines.extend(group("Delete files made since", &plan.removed, &said));
    if lines.is_empty() {
        lines.push(line(
            "Nothing to change",
            "Your files already match".to_owned(),
            said.clone(),
        ));
    }
    lines.push(line(
        "Note",
        "Files this folder ignores are not covered.".to_owned(),
        said.clone(),
    ));
    lines.push(line(
        "Note",
        "The agent will not know about this.".to_owned(),
        said,
    ));
    lines.into_iter().flatten().collect()
}
