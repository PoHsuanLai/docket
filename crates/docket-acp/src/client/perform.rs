//! What `Performer::perform` does for each action of the pseudo-app once the router has allowed
//! the call: read the held file, write the held text, start the held command, note what the
//! agent reported, take a write back. Each checks that the call it was handed names exactly what
//! the held request was formed for.

use super::call::StageId;
use super::files::FileFault;
use super::files::Files;
use super::performer::{Held, Performer, Scope, UNDO_KEEP, UndoNote};
use docket_core::{
    AbsPath, AppRefusal, FILES_READ, FILES_SENSITIVE, FILES_WRITE, FailText, Follow, Invocation,
    LabelText, Outcome, ParamName, PermissionKind, Preview, REPORTED, TERMINAL_RUN, TargetValue,
    UndoFault, UndoToken, Undoable, Value,
};
use docket_shell::Sandbox;
use porter_core::DataClass;
use prov::{Integrity, Label, Labelled, Source};
use serde_json::Value as Json;
use std::collections::BTreeSet;

fn failed(why: &str) -> AppRefusal {
    AppRefusal::Failed(FailText(why.to_owned()))
}

fn param<'a>(inv: &'a Invocation, name: &str) -> Option<&'a Value> {
    ParamName::parse(name)
        .ok()
        .and_then(|n| inv.args.get(&n))
        .map(|a| &a.value)
}

fn stage_of(inv: &Invocation) -> Result<StageId, AppRefusal> {
    match param(inv, "stage") {
        Some(Value::Text(t)) => StageId::parse(t).ok_or_else(|| failed("no such held request")),
        _ => Err(failed("the call names no held request")),
    }
}

fn lines_of(content: &str, from: Option<u32>, limit: Option<u32>) -> String {
    let skip = from.map_or(0, |n| n.saturating_sub(1)) as usize;
    let take = limit.map_or(usize::MAX, |n| n as usize);
    if skip == 0 && take == usize::MAX {
        return content.to_owned();
    }
    content
        .split_inclusive('\n')
        .skip(skip)
        .take(take)
        .collect()
}

fn file_failed(fault: FileFault) -> AppRefusal {
    failed(&format!("the file call failed: {fault}"))
}

fn done() -> Outcome {
    Outcome {
        value: None,
        said: None,
        show: Preview::None,
        undo: Undoable::No,
        follow: Follow::Nothing,
    }
}

/// What a call the agent only reported brings into the session: a read or a fetch or a command
/// is untrusted content in the agent's hands.
fn brings_content(what: &str) -> Option<Source> {
    match what {
        "read" | "search" | "execute" | "other" => Some(Source::File),
        "fetch" => Some(Source::Web),
        _ => None,
    }
}

impl<F: Files, S: Sandbox> Performer<F, S> {
    /// Performs a call the router allowed.
    pub fn perform(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        match inv.action.as_str() {
            FILES_READ => self.read(inv),
            FILES_WRITE | FILES_SENSITIVE => self.write(inv),
            TERMINAL_RUN => self.run(inv),
            REPORTED => Ok(self.reported(inv)),
            other if PermissionKind::ALL.iter().any(|k| k.action_name() == other) => Ok(done()),
            _ => Err(AppRefusal::Unsupported),
        }
    }

    /// Takes the staged request for `inv`, which must be the very thing it was formed for.
    fn take(&self, inv: &Invocation) -> Result<(Held, Scope), AppRefusal> {
        let stage = stage_of(inv)?;
        let mut inner = self.lock();
        let held = inner
            .held
            .remove(&stage)
            .ok_or_else(|| failed("no such held request"))?;
        let session = match &held {
            Held::Read { session, .. }
            | Held::Write { session, .. }
            | Held::Run { session, .. } => session.clone(),
        };
        let scope = inner
            .scopes
            .get(&session)
            .cloned()
            .ok_or_else(|| failed("the session is over"))?;
        Ok((held, scope))
    }

    fn only_file(inv: &Invocation) -> Option<AbsPath> {
        match &inv.target {
            TargetValue::Files(files) => match files.as_slice() {
                [one] => AbsPath::parse(one.as_str()).ok(),
                _ => None,
            },
            _ => None,
        }
    }

    fn read(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let (held, scope) = self.take(inv)?;
        let Held::Read {
            file, line, limit, ..
        } = held
        else {
            return Err(failed("the held request is not a read"));
        };
        if Self::only_file(inv) != Some(file.path.clone()) {
            return Err(failed("the call does not match its held request"));
        }
        let content = self
            .lock()
            .files
            .read(&file.real, &scope.real_cwd)
            .map_err(file_failed)?;
        let text = lines_of(&content, line, limit);
        // What the file held is somebody's words, in the agent's hands now: untrusted, and the
        // router takes the session's taint from this label.
        let label = Label::untrusted(Source::File, DataClass::Files, inv.space.clone());
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Text(text),
                label,
            }),
            ..done()
        })
    }

    fn write(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let (held, scope) = self.take(inv)?;
        let Held::Write { file, content, .. } = held else {
            return Err(failed("the held request is not a write"));
        };
        if Self::only_file(inv) != Some(file.path.clone()) {
            return Err(failed("the call does not match its held request"));
        }
        let mut inner = self.lock();
        let before = inner
            .files
            .write(&file.real, &scope.real_cwd, &content)
            .map_err(file_failed)?;
        inner.next += 1;
        let token =
            UndoToken::parse(&format!("u-{}", inner.next)).map_err(|_| failed("no undo token"))?;
        inner.undo.push((
            token.clone(),
            UndoNote {
                path: file.real.clone(),
                before,
            },
        ));
        if inner.undo.len() > UNDO_KEEP {
            inner.undo.remove(0);
        }
        Ok(Outcome {
            said: LabelText::parse("Wrote a file").ok(),
            undo: Undoable::Yes(token),
            ..done()
        })
    }

    fn run(&self, inv: &Invocation) -> Result<Outcome, AppRefusal> {
        let (held, scope) = self.take(inv)?;
        let Held::Run {
            line, cwd, params, ..
        } = held
        else {
            return Err(failed("the held request is not a command"));
        };
        let asked_line = matches!(param(inv, "command"), Some(Value::Text(t)) if *t == line);
        let asked_cwd = matches!(
            param(inv, "cwd"),
            Some(Value::File(f)) if f.as_str() == cwd.as_str()
        );
        if !asked_line || !asked_cwd {
            return Err(failed("the call does not match its held request"));
        }
        let answer = self
            .lock()
            .terminals
            .create(&scope.cwd, params)
            .map_err(|e| failed(&e.message))?;
        let id = answer
            .get("terminalId")
            .and_then(Json::as_str)
            .ok_or_else(|| failed("the terminal has no name"))?;
        let label = Label {
            integrity: Integrity::Trusted,
            confidentiality: prov::Confidentiality::Public,
            classes: BTreeSet::new(),
            sources: BTreeSet::from([Source::App(
                docket_core::acp_agent_app().ok_or_else(|| failed("no such app"))?,
            )]),
        };
        Ok(Outcome {
            value: Some(Labelled {
                value: Value::Text(id.to_owned()),
                label,
            }),
            ..done()
        })
    }

    fn reported(&self, inv: &Invocation) -> Outcome {
        let source = match param(inv, "what") {
            Some(Value::Text(what)) => brings_content(what),
            _ => Some(Source::File),
        };
        let value = source.map(|source| Labelled {
            value: Value::Text("noted".to_owned()),
            label: Label::untrusted(source, DataClass::Files, inv.space.clone()),
        });
        Outcome { value, ..done() }
    }

    /// Takes a write back: the file as it was, or gone if the write made it.
    pub fn undo(&self, token: &UndoToken) -> Result<(), UndoFault> {
        let mut inner = self.lock();
        let at = inner
            .undo
            .iter()
            .position(|(t, _)| t == token)
            .ok_or(UndoFault::Gone)?;
        let (_, note) = inner.undo.remove(at);
        let within = inner
            .scopes
            .values()
            .find(|s| s.real_cwd.covers(&note.path) == docket_core::Cover::Covers)
            .map(|s| s.real_cwd.clone())
            .ok_or(UndoFault::Gone)?;
        let done = match &note.before {
            Some(text) => inner.files.write(&note.path, &within, text).map(|_| ()),
            None => inner.files.remove(&note.path, &within),
        };
        done.map_err(|_| UndoFault::Gone)
    }
}
