//! `FakeMenu`: a scripted app whose `menu.item.activate` classifies each call, for the tests of
//! per-call effects. A test says, per item, what `Classify` answers, what the item is by the
//! time `Perform` runs, and what `Perform` returns.

use crate::labels::outcome;
use docket_client::IntentProvider;
use docket_core::{
    ActionRef, AppRefusal, CallClass, CallRequest, ClassifyFault, FailText, Follow, Hit,
    Invocation, Origin, Outcome, ParamName, Preview, SuggestAsk, TargetValue, UndoFault, UndoToken,
    Undoable, ValidManifest, Value,
};
use porter_core::AppName;
use prov::{Actor, Effect, EntityId};
use std::collections::BTreeMap;
use std::sync::Mutex;

/// What `Perform` does for an item once it has accepted the classification.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuPerform {
    /// Does it and says so.
    Done,
    /// Answers a follow-up call for this action of the app (a delegation).
    Follows(String),
    /// Fails.
    Fails,
}

/// How one menu item behaves.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MenuItem {
    /// What `Classify` answers.
    pub classify: Result<CallClass, ClassifyFault>,
    /// What the item is when `Perform` re-derives it; `None` means unchanged.
    pub at_perform: Option<CallClass>,
    /// What `Perform` does.
    pub perform: MenuPerform,
}

impl MenuItem {
    /// An item classified as `effect` that stays so.
    pub fn effect(effect: Effect) -> Self {
        Self {
            classify: Ok(CallClass::Effect(effect)),
            at_perform: None,
            perform: MenuPerform::Done,
        }
    }

    /// An item that delegates to `action` of the app.
    pub fn delegating(action: &str) -> Self {
        Self {
            classify: Ok(CallClass::Delegates(ActionRef {
                app: app_name(),
                name: prov::ActionName::parse(action).unwrap_or_else(|_| unreachable!()),
            })),
            at_perform: None,
            perform: MenuPerform::Follows(action.to_owned()),
        }
    }
}

/// Whether `action` is one of the `<app>.item.*` actions, whatever the app is called (a test
/// renames the manifest's app to stand in for another one).
fn is_item(action: &str) -> bool {
    action.split('.').nth(1) == Some("item")
}

fn app_name() -> AppName {
    AppName::parse("org.quire.Menu").unwrap_or_else(|_| unreachable!("a valid app name"))
}

/// The fake menu app.
#[derive(Debug)]
pub struct FakeMenu {
    manifest: ValidManifest,
    items: Mutex<BTreeMap<String, MenuItem>>,
    classified: Mutex<Vec<String>>,
}

impl FakeMenu {
    /// A menu app with this manifest and no items.
    pub fn new(manifest: ValidManifest) -> Self {
        Self {
            manifest,
            items: Mutex::new(BTreeMap::new()),
            classified: Mutex::new(Vec::new()),
        }
    }

    /// Forgets every scripted item and every item asked about.
    pub fn clear(&self) {
        if let Ok(mut items) = self.items.lock() {
            items.clear();
        }
        if let Ok(mut classified) = self.classified.lock() {
            classified.clear();
        }
    }

    /// Sets what `item` does.
    pub fn script(&self, item: &str, how: MenuItem) {
        if let Ok(mut items) = self.items.lock() {
            items.insert(item.to_owned(), how);
        }
    }

    /// The items `Classify` was asked about, in order.
    pub fn classified(&self) -> Vec<String> {
        self.classified
            .lock()
            .map(|c| c.clone())
            .unwrap_or_default()
    }

    fn item_of(inv: &Invocation) -> Option<String> {
        let name = ParamName::parse("item").ok()?;
        match &inv.args.get(&name)?.value {
            Value::Text(t) => Some(t.clone()),
            _ => None,
        }
    }

    fn script_of(&self, item: &str) -> Option<MenuItem> {
        self.items.lock().ok().and_then(|i| i.get(item).cloned())
    }
}

impl IntentProvider for FakeMenu {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        self.perform_classified(inv, None, None).await
    }

    async fn perform_classified(
        &self,
        inv: Invocation,
        _activation: Option<docket_core::ActivationToken>,
        classified: Option<CallClass>,
    ) -> Result<Outcome, AppRefusal> {
        if !is_item(inv.action.as_str()) {
            return Ok(outcome(
                Some(format!("Did {}", inv.action.as_str())),
                Undoable::No,
                Preview::None,
            ));
        }
        let item = Self::item_of(&inv).ok_or(AppRefusal::Unsupported)?;
        let how = self.script_of(&item).ok_or(AppRefusal::Unsupported)?;
        // Re-derive: what the item is now. The declared ceiling is always accepted.
        let now = how.at_perform.clone().or_else(|| how.classify.clone().ok());
        let ceiling = CallClass::Effect(if inv.action.as_str().ends_with(".item.adjust") {
            Effect::UndoableWrite
        } else {
            Effect::Destructive
        });
        if let Some(said) = classified
            && said != ceiling
            && Some(&said) != now.as_ref()
        {
            return Err(AppRefusal::ClassificationChanged);
        }
        match how.perform {
            MenuPerform::Done => Ok(outcome(
                Some(format!("Activated {item}")),
                Undoable::No,
                Preview::None,
            )),
            MenuPerform::Fails => Err(AppRefusal::Failed(FailText("no".into()))),
            MenuPerform::Follows(action) => {
                let mut out = outcome(None, Undoable::No, Preview::None);
                out.follow = Follow::Next(CallRequest {
                    action: ActionRef {
                        app: app_name(),
                        name: prov::ActionName::parse(&action)
                            .map_err(|_| AppRefusal::Unsupported)?,
                    },
                    target: TargetValue::Nothing,
                    args: Default::default(),
                    origin: Origin::AppInternal,
                });
                Ok(out)
            }
        }
    }

    async fn classify(&self, inv: Invocation) -> Result<CallClass, AppRefusal> {
        let item = Self::item_of(&inv).ok_or(AppRefusal::Unsupported)?;
        if let Ok(mut seen) = self.classified.lock() {
            seen.push(item.clone());
        }
        let how = self.script_of(&item).ok_or(AppRefusal::Unsupported)?;
        how.classify.map_err(|_| AppRefusal::Unsupported)
    }

    async fn dry_run(&self, _: Invocation) -> Result<Preview, AppRefusal> {
        Err(AppRefusal::Unsupported)
    }

    async fn undo(&self, _: UndoToken, _: Actor) -> Result<(), UndoFault> {
        Err(UndoFault::Gone)
    }

    async fn search(&self, _: &str) -> Vec<Hit> {
        vec![]
    }

    async fn preview(&self, _: &EntityId) -> Preview {
        Preview::None
    }

    async fn suggest(&self, _: SuggestAsk) -> Vec<docket_core::EntityRef> {
        vec![]
    }
}
