//! Which app and which action a word names, from the installed manifests: the app by its slug
//! (`mail`, the last element of its name, lowercased) or its full name, the action by its name
//! without the app's prefix (`thread.archive`) or in full (`mail.thread.archive`). An action the
//! app keeps for the person (`reach = hidden`) is not listed or described, but a call that
//! names it by hand is sent on: the router is the one that refuses it.

use crate::exit::Failure;
use docket_core::{ActionDecl, AgentReach, Manifest, ValidManifest, action_prefix};
use porter_core::AppName;
use prov::{ActionName, EntityKind};

/// The installed apps, as the registry listed them.
#[derive(Debug, Clone)]
pub struct Apps(Vec<ValidManifest>);

impl Apps {
    /// The apps `manifests` declare, in a stable order.
    pub fn new(mut manifests: Vec<ValidManifest>) -> Self {
        manifests.sort_by(|a, b| a.manifest().app.cmp(&b.manifest().app));
        Self(manifests)
    }

    /// Every app.
    pub fn all(&self) -> &[ValidManifest] {
        &self.0
    }

    /// The app a word names.
    pub fn app(&self, word: &str) -> Result<&Manifest, Failure> {
        let named: Vec<&Manifest> = self
            .0
            .iter()
            .map(ValidManifest::manifest)
            .filter(|m| m.app.as_str() == word || slug(&m.app) == word)
            .collect();
        match named.as_slice() {
            [one] => Ok(one),
            [] => Err(Failure::usage(format!(
                "no installed app is called {word:?} (see: quire-do apps)"
            ))),
            _ => Err(Failure::usage(format!(
                "{word:?} names several apps: use the full name"
            ))),
        }
    }

    /// The action of `app` a word names, hidden or not.
    pub fn action<'a>(&self, app: &'a Manifest, word: &str) -> Result<&'a ActionDecl, Failure> {
        let full = if word.starts_with(&format!("{}.", slug(&app.app))) {
            word.to_owned()
        } else {
            format!("{}.{word}", slug(&app.app))
        };
        let name = ActionName::parse(&full).ok();
        app.actions
            .iter()
            .find(|a| Some(&a.name) == name.as_ref())
            .ok_or_else(|| {
                Failure::usage(format!(
                    "{} has no action {word:?} (see: quire-do {} --list)",
                    slug(&app.app),
                    slug(&app.app)
                ))
            })
    }

    /// The app that declares `kind`, preferring `prefer`: an entity argument names a thing of
    /// some app, and the id carries it.
    pub fn owner_of(&self, kind: &EntityKind, prefer: &AppName) -> Option<AppName> {
        let declares = |m: &Manifest| m.entities.iter().any(|e| &e.kind == kind);
        self.0
            .iter()
            .map(ValidManifest::manifest)
            .find(|m| &m.app == prefer && declares(m))
            .or_else(|| {
                self.0
                    .iter()
                    .map(ValidManifest::manifest)
                    .find(|m| declares(m))
            })
            .map(|m| m.app.clone())
    }
}

/// An app's slug: what `quire-do` calls it.
pub fn slug(app: &AppName) -> String {
    action_prefix(app)
}

/// The action's name as typed after the app: without the app's prefix.
pub fn short_name(app: &Manifest, action: &ActionDecl) -> String {
    let prefix = format!("{}.", slug(&app.app));
    action
        .name
        .as_str()
        .strip_prefix(&prefix)
        .unwrap_or(action.name.as_str())
        .to_owned()
}

/// The actions an app offers a terminal: every one that is not hidden.
pub fn visible(app: &Manifest) -> impl Iterator<Item = &ActionDecl> {
    app.actions.iter().filter(|a| a.reach != AgentReach::Hidden)
}
