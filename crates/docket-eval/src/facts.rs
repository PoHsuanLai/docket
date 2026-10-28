//! What the shipped fake apps' manifests say about an action, for the scores that need to know
//! whether a call only reads or what its sheet is called.

use docket_context::registry::Registry;
use docket_vocab::ActionRef;
use prov::Effect;

/// The registry of the fake apps, or nothing when it cannot be built (then every action is
/// unknown, which scores as acting and labels nothing).
#[derive(Debug)]
pub struct Facts(Option<Registry>);

impl Facts {
    /// The facts of the fake apps' manifests.
    pub fn shipped() -> Self {
        Self(docket_fake::registry().ok())
    }

    /// What `action` does to the world, if the registry knows it.
    pub fn effect(&self, action: &ActionRef) -> Option<Effect> {
        self.0.as_ref()?.action(action).map(|decl| decl.effect)
    }

    /// The label a sheet gives `action`, if the registry knows it.
    pub fn label(&self, action: &ActionRef) -> Option<String> {
        let decl = self.0.as_ref()?.action(action)?;
        Some(decl.label.as_str().to_owned())
    }
}
