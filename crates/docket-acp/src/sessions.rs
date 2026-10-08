//! `session/new`, `session/load` and `session/list`. An editor sees and restores only the sessions
//! it opened: the same rule the router applies to a stored session (`may_restore`, the editor
//! under `Own`), asked here before anything is listed or replayed, and answered with the same
//! "no such session" whether the session is another app's or does not exist.

use crate::fault;
use crate::history;
use crate::mode::Mode;
use crate::out;
use crate::server::{Known, Server, Ticks};
use crate::wire::Wire;
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, Error, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, SessionNotification,
};
use docket_core::CallerRole;
use docket_session::{
    BackendKind, Claimant, Opening, ResumePlan, SessionHost, SessionLog, Workspace, may_restore,
    read_all, resume_plan,
};
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use serde_json::Value;
use std::path::Path;

/// Sessions to a page of `session/list`.
const PAGE: usize = 50;

fn workspace(path: &Path) -> Result<Workspace, Error> {
    path.to_str()
        .and_then(|p| Workspace::parse(p).ok())
        .ok_or_else(|| fault::invalid("cwd must be an absolute path"))
}

fn json(of: &impl serde::Serialize) -> Result<Value, Error> {
    serde_json::to_value(of).map_err(|_| fault::internal("could not answer"))
}

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks> Server<H, L, W, T> {
    /// The plan of a stored session this editor may use, or "no such session".
    async fn mine(
        &self,
        session: &SessionId,
    ) -> Result<(Vec<docket_session::Logged>, ResumePlan), Error> {
        let rows = read_all(&self.log, session)
            .await
            .map_err(|_| fault::internal("the session log could not be read"))?;
        let plan = resume_plan(&rows).map_err(|_| fault::unknown_session())?;
        let claim = Claimant {
            role: CallerRole::Editor,
            app: &self.editor,
        };
        may_restore(claim, plan.opening.opener.as_ref())
            .then_some((rows, plan))
            .ok_or_else(fault::unknown_session)
    }

    pub(crate) async fn new_session(&mut self, params: Value) -> Result<Value, Error> {
        // The editor's `mcpServers` are read and dropped: the editor's tools are not ours to run.
        let asked: NewSessionRequest = fault::params(params)?;
        let cwd = workspace(&asked.cwd)?;
        let n = self.mint();
        let opening = Opening {
            task: TaskId::parse(&format!("acp-t-{n}")).map_err(|_| fault::internal("no task"))?,
            space: SpaceId::desktop(),
            opener: Some(self.editor.clone()),
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Native,
            parent: None,
            forked_from: None,
            cwd: Some(cwd.clone()),
            started_from: None,
            label: None,
        };
        let session = self
            .host
            .open(opening)
            .await
            .map_err(|_| fault::internal("the session could not be opened"))?;
        let mode = Mode::default();
        self.roster.insert(session.clone(), Known { mode });
        json(&NewSessionResponse::new(out::wire_id(&session)).modes(mode.state()))
    }

    pub(crate) async fn load_session(&mut self, params: Value) -> Result<Value, Error> {
        let asked: LoadSessionRequest = fault::params(params)?;
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        let (rows, plan) = self.mine(&session).await?;
        let cwd = workspace(&asked.cwd)?;
        if plan.opening.cwd.as_ref() != Some(&cwd) {
            return Err(fault::invalid("cwd does not match the session's"));
        }
        for update in history::replay(&rows, &plan) {
            let note = SessionNotification::new(out::wire_id(&session), update);
            let line = out::notify(CLIENT_METHOD_NAMES.session_update, &note);
            self.wire
                .write_line(line)
                .await
                .map_err(|_| fault::internal("the connection closed"))?;
        }
        if !self.roster.contains_key(&session) {
            self.host
                .resume(&session)
                .await
                .map_err(|_| fault::internal("the session could not be resumed"))?;
        }
        let mode = Mode::default();
        self.roster.insert(session, Known { mode });
        json(&LoadSessionResponse::new().modes(mode.state()))
    }

    pub(crate) async fn list_sessions(&mut self, params: Value) -> Result<Value, Error> {
        let asked: ListSessionsRequest = fault::params(params)?;
        let wanted = asked.cwd.as_deref().map(workspace).transpose()?;
        let skip: usize = match asked.cursor.as_deref() {
            None => 0,
            Some(c) => c.parse().map_err(|_| fault::invalid("bad cursor"))?,
        };
        let stored = self
            .log
            .sessions()
            .await
            .map_err(|_| fault::internal("the session log could not be read"))?;
        let mut found = Vec::new();
        for session in stored {
            let Ok((_, plan)) = self.mine(&session).await else {
                continue;
            };
            if wanted
                .as_ref()
                .is_some_and(|w| plan.opening.cwd.as_ref() != Some(w))
            {
                continue;
            }
            if let Some(info) = history::info(&session, &plan) {
                found.push((history::last_spoken(&plan), session, info));
            }
        }
        // Newest first; the id breaks a tie so the order is the same every time.
        found.sort_by(|a, b| (b.0, &b.1).cmp(&(a.0, &a.1)));
        let page: Vec<_> = found
            .iter()
            .skip(skip)
            .take(PAGE)
            .map(|f| f.2.clone())
            .collect();
        let next = (skip + PAGE < found.len()).then(|| (skip + PAGE).to_string());
        json(&ListSessionsResponse::new(page).next_cursor(next))
    }
}
