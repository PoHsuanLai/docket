//! `session/new`, `session/load` and `session/list`. An editor sees and restores only the sessions
//! it opened, and a helper's conversation: the same rule the router applies to a stored session
//! (`may_restore_plan`, the editor under `Own` plus the conversation seat), asked here before anything is listed or replayed, and answered with the same
//! "no such session" whether the session is another app's or does not exist.

use crate::helper_face::{helper_asked, space_of};
use crate::helpers::Helpers;
use crate::history;
use crate::link::Link;
use crate::mode::Mode;
use crate::out;
use crate::server::{Known, Server, Turns};
use acp_core::fault;
use agent_client_protocol::schema::v1::{
    Error, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest, LoadSessionResponse,
    NewSessionRequest, NewSessionResponse, SessionInfo,
};
use docket_core::CallerRole;
use docket_session::{
    BackendKind, Claimant, Opening, ResumePlan, SessionHost, SessionLog, may_restore_plan,
    read_all, resume_plan,
};
use docket_vocab::Clock;
use docket_vocab::Workspace;
use prov::{AgentRef, SessionId, TaskId};
use std::path::Path;

/// Sessions to a page of `session/list`.
const PAGE: usize = 50;

pub(crate) fn workspace(path: &Path) -> Result<Workspace, Error> {
    path.to_str()
        .and_then(|p| Workspace::parse(p).ok())
        .ok_or_else(|| fault::invalid("cwd must be an absolute path"))
}

impl<H: SessionHost, L: SessionLog, T: Clock, S: Helpers> Server<H, L, T, S> {
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
        may_restore_plan(claim, &plan)
            .then_some((rows, plan))
            .ok_or_else(fault::unknown_session)
    }

    pub(crate) async fn new_session(
        &mut self,
        asked: NewSessionRequest,
    ) -> Result<NewSessionResponse, Error> {
        // The editor's `mcpServers` are read and dropped: the editor's tools are not ours to run.
        // `_meta.helper` names a helper to open a job for; without it the session is the companion.
        let helper = helper_asked(asked.meta.as_ref());
        let cwd = workspace(&asked.cwd)?;
        if let Some(name) = helper {
            return self.new_helper_job(&name, &cwd).await;
        }
        let n = self.mint();
        let opening = Opening {
            task: TaskId::parse(&format!("acp-t-{n}")).map_err(|_| fault::internal("no task"))?,
            space: space_of(&cwd),
            opener: Some(self.editor.clone()),
            agent: Some(AgentRef::Companion),
            backend: BackendKind::Native,
            parent: None,
            forked_from: None,
            cwd: Some(cwd.clone()),
            started_from: None,
            label: None,
            rewind: Default::default(),
        };
        let session = self
            .host
            .open(opening)
            .await
            .map_err(|_| fault::internal("the session could not be opened"))?;
        let mode = Mode::default();
        let (helper, turns) = (None, Turns::Here);
        self.roster.insert(
            session.clone(),
            Known {
                mode,
                helper,
                turns,
            },
        );
        Ok(NewSessionResponse::new(out::wire_id(&session)).modes(mode.state()))
    }

    pub(crate) async fn load_session(
        &mut self,
        link: &Link,
        asked: LoadSessionRequest,
    ) -> Result<LoadSessionResponse, Error> {
        let session =
            SessionId::parse(&asked.session_id.0).map_err(|_| fault::unknown_session())?;
        // A helper's conversation is not a stored session of this editor's: it is found by name
        // in the workspace's Space.
        let here = workspace(&asked.cwd).ok();
        let helper = here
            .as_ref()
            .and_then(|cwd| self.helper_session(&session, cwd));
        if let Some(row) = helper {
            return self.load_helper(link, row).await;
        }
        let (rows, plan) = self.mine(&session).await?;
        let cwd = workspace(&asked.cwd)?;
        if plan.opening.cwd.as_ref() != Some(&cwd) {
            return Err(fault::invalid("cwd does not match the session's"));
        }
        for update in history::replay(&rows, &plan) {
            link.update(&session, update)
                .map_err(|_| fault::internal("the connection closed"))?;
        }
        if !self.roster.contains_key(&session) {
            self.host
                .resume(&session)
                .await
                .map_err(|why| {
                tracing::warn!(?why, %session, "session/load: the host could not resume the session");
                fault::internal("the session could not be resumed")
            })?;
        }
        let mode = Mode::default();
        let (helper, turns) = (None, Turns::Here);
        self.roster.insert(
            session,
            Known {
                mode,
                helper,
                turns,
            },
        );
        Ok(LoadSessionResponse::new().modes(mode.state()))
    }

    pub(crate) async fn list_sessions(
        &mut self,
        asked: ListSessionsRequest,
    ) -> Result<ListSessionsResponse, Error> {
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
        let mut infos: Vec<SessionInfo> = wanted
            .as_ref()
            .map(|w| self.helper_infos(w))
            .unwrap_or_default();
        let helpers: Vec<SessionId> = wanted
            .as_ref()
            .map(|w| {
                self.helpers_in(w)
                    .into_iter()
                    .map(|row| row.session)
                    .collect()
            })
            .unwrap_or_default();
        let mut found = Vec::new();
        for session in stored {
            if helpers.contains(&session) {
                continue;
            }
            let Ok((_, plan)) = self.mine(&session).await else {
                continue;
            };
            // A helper's conversation is listed with the helpers, not as a session of its own.
            if plan.seat.is_some() {
                continue;
            }
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
        // The helpers come first, beside the companion's sessions.
        infos.extend(found.into_iter().map(|f| f.2));
        let page: Vec<_> = infos.iter().skip(skip).take(PAGE).cloned().collect();
        let next = (skip + PAGE < infos.len()).then(|| (skip + PAGE).to_string());
        Ok(ListSessionsResponse::new(page).next_cursor(next))
    }
}
