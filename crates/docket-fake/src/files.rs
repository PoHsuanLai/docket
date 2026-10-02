//! `FakeFiles`: an in-memory files app behind the fixture manifest, with real undo tokens.

use crate::labels::{entity, outcome, third_party};
use docket_client::IntentProvider;
use docket_core::{
    AppRefusal, FileMove, FileRef, Hit, Invocation, Outcome, ParamName, Preview, SuggestAsk,
    TargetValue, UndoFault, UndoToken, Undoable, ValidManifest, Value,
};
use porter_core::{AppName, DataClass};
use prov::{Actor, EntityId, Labelled, Source, SpaceId};
use std::collections::BTreeMap;
use std::sync::Mutex;

#[derive(Debug, Default)]
struct State {
    /// Where each file lives: key to path.
    files: BTreeMap<String, String>,
    content: BTreeMap<String, String>,
    undo: BTreeMap<String, Vec<(String, String)>>,
    next: u64,
}

/// The fake files app.
#[derive(Debug)]
pub struct FakeFiles {
    manifest: ValidManifest,
    app: AppName,
    space: SpaceId,
    state: Mutex<State>,
}

impl FakeFiles {
    /// A files app with this manifest, working in `space`.
    pub fn new(manifest: ValidManifest, space: SpaceId) -> Self {
        let app = manifest.manifest().app.clone();
        Self {
            manifest,
            app,
            space,
            state: Mutex::new(State::default()),
        }
    }

    /// Adds a file at `path` with `content`.
    pub fn with_file(self, key: &str, path: &str, content: &str) -> Self {
        self.edit(|s| {
            s.files.insert(key.into(), path.into());
            s.content.insert(key.into(), content.into());
        });
        self
    }

    fn edit<R>(&self, f: impl FnOnce(&mut State) -> R) -> R {
        match self.state.lock() {
            Ok(mut s) => f(&mut s),
            Err(p) => f(&mut p.into_inner()),
        }
    }

    /// Where a file is now.
    pub fn path_of(&self, key: &str) -> Option<String> {
        self.edit(|s| s.files.get(key).cloned())
    }

    fn keys(target: &TargetValue) -> Vec<String> {
        match target {
            TargetValue::Entities(ids) => ids.iter().map(|e| e.key.as_str().to_owned()).collect(),
            TargetValue::Nothing | TargetValue::Text(_) | TargetValue::Files(_) => vec![],
        }
    }

    fn destination(inv: &Invocation) -> Option<String> {
        let name = ParamName::parse("to").ok()?;
        match &inv.args.get(&name)?.value {
            Value::File(f) => Some(f.as_str().to_owned()),
            _ => None,
        }
    }
}

impl IntentProvider for FakeFiles {
    fn manifest(&self) -> &ValidManifest {
        &self.manifest
    }

    async fn perform(&self, inv: Invocation) -> Result<Outcome, AppRefusal> {
        let keys = Self::keys(&inv.target);
        let (app, space) = (self.app.clone(), self.space.clone());
        self.edit(|s| match inv.action.as_str() {
            "files.file.read" => {
                let key = keys.first().ok_or(AppRefusal::Unsupported)?;
                let text = s.content.get(key).ok_or_else(|| {
                    entity(&app, "files.file", key)
                        .map_or(AppRefusal::Unsupported, AppRefusal::NotFound)
                })?;
                let label = third_party(Source::File, DataClass::Files, space);
                let mut out = outcome(None, Undoable::No, Preview::None);
                out.value = Some(Labelled {
                    value: Value::Text(text.clone()),
                    label,
                });
                Ok(out)
            }
            "files.file.move" => {
                let to = Self::destination(&inv).ok_or(AppRefusal::Unsupported)?;
                let mut undo = Vec::new();
                for k in &keys {
                    let from = s.files.get(k).cloned().ok_or(AppRefusal::Unsupported)?;
                    undo.push((k.clone(), from));
                    s.files.insert(k.clone(), format!("{to}/{k}"));
                }
                s.next += 1;
                let token = UndoToken::parse(&format!("files-undo-{}", s.next))
                    .map_err(|_| AppRefusal::Busy)?;
                s.undo.insert(token.as_str().to_owned(), undo);
                Ok(outcome(
                    Some(format!("Moved {} files", keys.len())),
                    Undoable::Yes(token),
                    Preview::None,
                ))
            }
            "files.file.delete" => {
                keys.iter().for_each(|k| {
                    s.files.remove(k);
                    s.content.remove(k);
                });
                Ok(outcome(
                    Some(format!("Deleted {} files", keys.len())),
                    Undoable::No,
                    Preview::None,
                ))
            }
            _ => Err(AppRefusal::Unsupported),
        })
    }

    async fn dry_run(&self, inv: Invocation) -> Result<Preview, AppRefusal> {
        let keys = Self::keys(&inv.target);
        self.edit(|s| match inv.action.as_str() {
            "files.file.move" => {
                let to = Self::destination(&inv).ok_or(AppRefusal::Unsupported)?;
                let moves = keys
                    .iter()
                    .filter_map(|k| {
                        let from = FileRef::parse(s.files.get(k)?).ok()?;
                        let dest = FileRef::parse(&format!("{to}/{k}")).ok()?;
                        Some(FileMove { from, to: dest })
                    })
                    .collect();
                Ok(Preview::Moves(moves))
            }
            _ => Ok(Preview::None),
        })
    }

    async fn undo(&self, token: UndoToken, _actor: Actor) -> Result<(), UndoFault> {
        self.edit(|s| match s.undo.remove(token.as_str()) {
            None => Err(UndoFault::Gone),
            Some(back) => {
                back.into_iter().for_each(|(k, from)| {
                    s.files.insert(k, from);
                });
                Ok(())
            }
        })
    }

    async fn search(&self, _text: &str) -> Vec<Hit> {
        // Files are not indexed by this fixture: the router asks the index, not the app.
        vec![]
    }

    async fn preview(&self, id: &EntityId) -> Preview {
        self.edit(|s| {
            match s
                .files
                .get(id.key.as_str())
                .and_then(|p| FileRef::parse(p).ok())
            {
                Some(file) => Preview::Image(file),
                None => Preview::None,
            }
        })
    }

    async fn suggest(&self, _ask: SuggestAsk) -> Vec<docket_core::EntityRef> {
        vec![]
    }
}
