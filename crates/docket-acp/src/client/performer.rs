//! What the host does once the router has allowed an agent's call: read the file, write it,
//! start the command. The router's `Perform` for the `org.quire.AcpAgent` pseudo-app comes here
//! (in a test through `HostedApp`, in the process through its `IntentProvider1`), so nothing in
//! this file runs unless the gate said yes.
//!
//! Every performing call carries a `stage`: a handle for the request the host formed and holds
//! (the text of a write, the argument vector and environment of a command), bound to the session
//! it was formed for. The router rules on the call's path or command line; this performs the
//! staged request only if the call it was handed matches it exactly, and spends it. A stage
//! cannot be used twice, by another session, or for another path.

use super::call::StageId;
use super::confine::Confined;
use super::files::{FileFault, Files};
use crate::terminals::Terminals;
use agent_client_protocol_schema::v1::{ClientCapabilities, Error};
use docket_core::{AbsPath, UndoToken};
use docket_shell::Sandbox;
use prov::SessionId;
use serde_json::Value as Json;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, MutexGuard};

/// The most undo notes kept in memory.
pub(super) const UNDO_KEEP: usize = 256;

/// A write that can be taken back: the text it replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoNote {
    /// The file.
    pub path: AbsPath,
    /// What was there; `None` if the write created it.
    pub before: Option<String>,
}

/// Where a session's calls may reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Scope {
    pub(super) cwd: AbsPath,
    pub(super) real_cwd: AbsPath,
}

/// A request the host formed and holds.
#[derive(Debug, Clone)]
pub(super) enum Held {
    Read {
        session: SessionId,
        file: Confined,
        line: Option<u32>,
        limit: Option<u32>,
    },
    Write {
        session: SessionId,
        file: Confined,
        content: String,
    },
    Run {
        session: SessionId,
        line: String,
        cwd: AbsPath,
        params: Json,
    },
}

pub(super) struct Inner<F, S: Sandbox> {
    pub(super) files: F,
    pub(super) terminals: Terminals<S>,
    pub(super) scopes: BTreeMap<SessionId, Scope>,
    pub(super) held: BTreeMap<StageId, Held>,
    pub(super) undo: Vec<(UndoToken, UndoNote)>,
    pub(super) next: u64,
}

/// The host's performer. Cloning shares it: the backend stages through one handle and the router's
/// `Perform` arrives at another.
pub struct Performer<F: Files, S: Sandbox> {
    inner: Arc<Mutex<Inner<F, S>>>,
}

impl<F: Files, S: Sandbox> Clone for Performer<F, S> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
        }
    }
}

impl<F: Files, S: Sandbox> std::fmt::Debug for Performer<F, S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Performer")
    }
}

impl<F: Files, S: Sandbox> Performer<F, S> {
    /// A performer over `files` and the terminal `sandbox`.
    pub fn new(files: F, sandbox: S) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                files,
                terminals: Terminals::new(sandbox),
                scopes: BTreeMap::new(),
                held: BTreeMap::new(),
                undo: Vec::new(),
                next: 0,
            })),
        }
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, Inner<F, S>> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// `base` with the terminal capability on exactly when the sandbox is there.
    pub fn capabilities(&self, base: ClientCapabilities) -> ClientCapabilities {
        self.lock().terminals.capabilities(base)
    }

    /// Where `path` really leads (links followed).
    pub fn real(&self, path: &AbsPath) -> Result<AbsPath, FileFault> {
        self.lock().files.real(path)
    }

    /// Whether a command can be confined in `cwd`.
    pub fn check(&self, cwd: &AbsPath) -> docket_core::SandboxState {
        self.lock().terminals.check(cwd)
    }

    /// The session works in `cwd`, which really is `real_cwd`.
    pub fn open_scope(&self, session: &SessionId, cwd: AbsPath, real_cwd: AbsPath) {
        self.lock()
            .scopes
            .insert(session.clone(), Scope { cwd, real_cwd });
    }

    /// The session is over: whatever it staged and never used is dropped.
    pub fn close_scope(&self, session: &SessionId) {
        let mut inner = self.lock();
        inner.scopes.remove(session);
        inner.held.retain(|_, held| match held {
            Held::Read { session: s, .. }
            | Held::Write { session: s, .. }
            | Held::Run { session: s, .. } => s != session,
        });
    }

    fn hold(inner: &mut Inner<F, S>, held: Held) -> StageId {
        inner.next += 1;
        let id = StageId::numbered(inner.next);
        inner.held.insert(id.clone(), held);
        id
    }

    /// Holds a read for `session`.
    pub fn stage_read(
        &self,
        session: &SessionId,
        file: Confined,
        line: Option<u32>,
        limit: Option<u32>,
    ) -> StageId {
        let mut inner = self.lock();
        let held = Held::Read {
            session: session.clone(),
            file,
            line,
            limit,
        };
        Self::hold(&mut inner, held)
    }

    /// Holds a write for `session`; the text stays here.
    pub fn stage_write(&self, session: &SessionId, file: Confined, content: String) -> StageId {
        let mut inner = self.lock();
        let held = Held::Write {
            session: session.clone(),
            file,
            content,
        };
        Self::hold(&mut inner, held)
    }

    /// Holds a command for `session`: its literal line, where it runs, and the whole request.
    pub fn stage_run(
        &self,
        session: &SessionId,
        line: String,
        cwd: AbsPath,
        params: Json,
    ) -> StageId {
        let mut inner = self.lock();
        let held = Held::Run {
            session: session.clone(),
            line,
            cwd,
            params,
        };
        Self::hold(&mut inner, held)
    }

    /// Drops a staged request the router did not allow, so it cannot be used later.
    pub fn drop_stage(&self, stage: &StageId) {
        self.lock().held.remove(stage);
    }

    /// The four `terminal/*` methods that need no ruling.
    pub fn terminal_other(&self, method: &str, params: Json) -> Option<Result<Json, Error>> {
        self.lock().terminals.handle(method, params)
    }

    /// `terminal/wait_for_exit` without blocking.
    pub fn poll_wait(&self, params: Json) -> Result<Option<Json>, Error> {
        self.lock().terminals.poll_wait(params)
    }

    /// The writes that can still be taken back, newest last (for a test).
    pub fn undo_notes(&self) -> Vec<UndoNote> {
        self.lock().undo.iter().map(|(_, n)| n.clone()).collect()
    }
}
