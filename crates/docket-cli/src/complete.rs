//! What a shell completes next. The completion files in `dist/completions` call
//! `quire-do __complete <words…>` and offer what it prints, so they stay in step with the
//! installed manifests: apps, actions, parameter flags and the options of a choice.

use crate::resolve::{Apps, short_name, slug, visible};
use docket_core::{Manifest, ParamType};

const COMMANDS: [&str; 5] = ["apps", "describe", "undo", "--help", "--version"];
const ACTIONLESS: [&str; 3] = ["--list", "search", "context"];
const GLOBALS: [&str; 3] = ["--dry-run", "--json", "--session"];

fn actions(app: &Manifest) -> Vec<String> {
    visible(app).map(|a| short_name(app, a)).collect()
}

fn flags(app: &Manifest, action: &str) -> Vec<String> {
    let Some(decl) = visible(app).find(|a| short_name(app, a) == action) else {
        return vec![];
    };
    decl.params
        .iter()
        .map(|p| format!("--{}", p.name.as_str().replace('_', "-")))
        .chain(GLOBALS.iter().map(|g| (*g).to_owned()))
        .collect()
}

fn options_of(app: &Manifest, action: &str, flag: &str) -> Vec<String> {
    let wanted = flag.trim_start_matches("--").replace('-', "_");
    visible(app)
        .find(|a| short_name(app, a) == action)
        .and_then(|a| a.params.iter().find(|p| p.name.as_str() == wanted))
        .map(|p| match &p.ty {
            ParamType::Choice(options) => options.iter().map(|o| o.id.to_string()).collect(),
            _ => vec![],
        })
        .unwrap_or_default()
}

fn slugs(apps: &Apps) -> Vec<String> {
    apps.all().iter().map(|m| slug(&m.manifest().app)).collect()
}

/// The words that may come after `typed`, whose last element is the word being completed.
pub fn candidates(apps: &Apps, typed: &[String]) -> Vec<String> {
    let (current, before) = match typed.split_last() {
        Some((current, before)) => (current.as_str(), before),
        None => ("", &[][..]),
    };
    let found: Vec<String> = match before {
        [] => COMMANDS
            .iter()
            .map(|c| (*c).to_owned())
            .chain(slugs(apps))
            .collect(),
        [first] if first == "describe" => slugs(apps),
        [first, app] if first == "describe" => apps.app(app).map(actions).unwrap_or_default(),
        [first] if first == "undo" => vec!["--last".to_owned()],
        [app] => match apps.app(app) {
            Ok(m) => ACTIONLESS
                .iter()
                .map(|w| (*w).to_owned())
                .chain(actions(m))
                .collect(),
            Err(_) => vec![],
        },
        [app, action, .., last] if last.starts_with("--") && !GLOBALS.contains(&last.as_str()) => {
            apps.app(app)
                .map(|m| options_of(m, action, last))
                .unwrap_or_default()
        }
        [app, action, ..] => apps.app(app).map(|m| flags(m, action)).unwrap_or_default(),
    };
    found
        .into_iter()
        .filter(|w| w.starts_with(current))
        .collect()
}
