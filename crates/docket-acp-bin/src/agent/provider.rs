//! The host's side of the `org.quire.AcpAgent` pseudo-app on the bus: intentd calls
//! `org.quire.IntentProvider1` here after the router allowed an agent's call, and the performer
//! does it. Only intentd may call a provider (`docket-client` enforces it), so nothing reaches
//! the performer that the gate did not allow.

use docket_acp::client::{Files, Performer};
use docket_client::{ContextSource, IntentProvider, SummonTarget};
use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, EntityRef, Here, Hit, Invocation, Outcome, Preview,
    Selection, SuggestAsk, SummonAnswer, SummonOrigin, SummonSerial, TextTarget, UndoFault,
    UndoToken, ValidManifest, Visible, WindowPrivacy,
};
use docket_shell::Sandbox;
use porter_core::{AppName, Count};
use prov::{Actor, Confidentiality, EntityId, Integrity, Label, Labelled, Source};
use std::collections::BTreeSet;

/// The performer, as an app's provider.
pub struct PerformerProvider<F: Files + 'static, S: Sandbox + 'static> {
    manifest: ValidManifest,
    performer: Performer<F, S>,
}

impl<F: Files + 'static, S: Sandbox + 'static> std::fmt::Debug for PerformerProvider<F, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PerformerProvider")
    }
}

impl<F: Files + 'static, S: Sandbox + 'static> PerformerProvider<F, S> {
    /// Serves `performer` under `manifest` (`manifests/org.quire.AcpAgent.toml`).
    pub fn new(manifest: ValidManifest, performer: Performer<F, S>) -> Self {
        Self {
            manifest,
            performer,
        }
    }
}

impl<F: Files + 'static, S: Sandbox + 'static> IntentProvider for PerformerProvider<F, S> {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.performer.perform(&inv)
    }

    async fn dry_run(&self, _inv: Invocation) -> Result<Preview, AppRefusal> {
        Err(AppRefusal::Unsupported)
    }

    async fn undo(&self, token: UndoToken, _actor: Actor) -> Result<(), UndoFault> {
        self.performer.undo(&token)
    }

    async fn search(&self, _text: &str) -> Vec<Hit> {
        Vec::new()
    }

    async fn preview(&self, _id: &EntityId) -> Preview {
        Preview::None
    }

    async fn suggest(&self, _ask: SuggestAsk) -> Vec<EntityRef> {
        Vec::new()
    }
}

/// The host has no window: its context is a quiet one, and it takes no summon.
#[derive(Debug, Clone)]
pub struct Quiet(pub AppName);

impl ContextSource for Quiet {
    fn snapshot(&self, _scope: ContextScope) -> ContextSnapshot {
        ContextSnapshot {
            app: self.0.clone(),
            window: Labelled {
                value: "Agent".to_owned(),
                label: Label {
                    integrity: Integrity::Trusted,
                    confidentiality: Confidentiality::Public,
                    classes: BTreeSet::new(),
                    sources: BTreeSet::from([Source::App(self.0.clone())]),
                },
            },
            here: Here::Nowhere,
            selection: Selection::Nothing,
            visible: Visible {
                kind: None,
                items: Vec::new(),
                total: Count(0),
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        }
    }
}

impl SummonTarget for Quiet {
    fn summon(&self, _serial: SummonSerial, _origin: SummonOrigin) -> SummonAnswer {
        SummonAnswer::Declined
    }
}
