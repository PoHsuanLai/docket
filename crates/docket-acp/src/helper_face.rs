//! The editor's face of the helpers (design note `swarm.md`, section 7.2): which helpers an
//! editor sees in a workspace, opening and loading their sessions, and the board's sentences
//! streamed as plain agent lines. A helper's turns, calls and sheets are the ordinary ones: its
//! session is in the same host, and the person's words to it are the editor's, with the `Editor`
//! role's caps, like any other prompt.

use crate::calls;
use crate::fault;
use crate::helper_replay;
use crate::helpers::{HelperRow, Helpers};
use crate::mode::Mode;
use crate::out;
use crate::server::{Known, Server, Ticks};
use crate::sessions::json;
use crate::wire::{Wire, WireClosed};
use agent_client_protocol_schema::v1::{
    CLIENT_METHOD_NAMES, Error, LoadSessionResponse, NewSessionResponse, SessionInfo,
    SessionNotification, SessionUpdate,
};
use companion_wire::ToldWire;
use docket_core::Workspace;
use docket_session::{SessionHost, SessionLog};
use prov::{HelperId, SessionId, SpaceId};
use serde_json::Value;

/// The Space a workspace maps to (owner decision D4). Every workspace maps to the desktop Space
/// until the per-workspace setting exists; sessions an editor opens and the helpers it lists use
/// this one function, so they cannot disagree.
pub(crate) fn space_of(_workspace: &Workspace) -> SpaceId {
    SpaceId::desktop()
}

/// The helper a `session/new` names, in its `_meta`.
pub(crate) fn helper_asked(params: &Value) -> Option<String> {
    params
        .pointer("/_meta/helper")
        .and_then(Value::as_str)
        .map(|name| name.trim().to_owned())
}

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks, S: Helpers> Server<H, L, W, T, S> {
    /// The helpers whose Space is the one `workspace` maps to.
    pub(crate) fn helpers_in(&self, workspace: &Workspace) -> Vec<HelperRow> {
        let space = space_of(workspace);
        self.helpers
            .rows()
            .into_iter()
            .filter(|row| row.space == space)
            .collect()
    }

    /// The `session/list` lines of those helpers, titled with their names.
    pub(crate) fn helper_infos(&self, workspace: &Workspace) -> Vec<SessionInfo> {
        self.helpers_in(workspace)
            .into_iter()
            .map(|row| {
                SessionInfo::new(out::wire_id(&row.session), workspace.as_str())
                    .title(Some(row.name.as_str().to_owned()))
            })
            .collect()
    }

    pub(crate) async fn send_update(
        &mut self,
        session: &SessionId,
        update: SessionUpdate,
    ) -> Result<(), WireClosed> {
        let note = SessionNotification::new(out::wire_id(session), update);
        let line = out::notify(CLIENT_METHOD_NAMES.session_update, &note);
        self.wire.write_line(line).await
    }

    /// `session/new` with `_meta.helper`: a job session for the helper called `name`.
    pub(crate) async fn new_helper_job(
        &mut self,
        name: &str,
        workspace: &Workspace,
    ) -> Result<Value, Error> {
        let row = self
            .helpers_in(workspace)
            .into_iter()
            .find(|row| row.name.as_str() == name)
            .ok_or_else(|| fault::invalid(&format!("There is no helper called {name} here.")))?;
        let session = self
            .helpers
            .open_job(&row.id)
            .await
            .map_err(|refusal| fault::invalid(&refusal.0))?;
        self.host
            .resume(&session)
            .await
            .map_err(|_| fault::internal("the job could not be started"))?;
        let mode = Mode::default();
        let helper = Some(row.id);
        self.roster.insert(session.clone(), Known { mode, helper });
        json(&NewSessionResponse::new(out::wire_id(&session)).modes(mode.state()))
    }

    /// The helper whose conversation `session` is, if the workspace's Space holds one.
    pub(crate) fn helper_session(
        &self,
        session: &SessionId,
        workspace: &Workspace,
    ) -> Option<HelperRow> {
        self.helpers_in(workspace)
            .into_iter()
            .find(|row| &row.session == session)
    }

    /// `session/load` of a helper's conversation: its recent jobs replayed, then the session
    /// ready for the person's turns.
    pub(crate) async fn load_helper(&mut self, row: HelperRow) -> Result<Value, Error> {
        let jobs = self.helpers.jobs(&row.id);
        for update in helper_replay::recent(&row.name, &jobs) {
            self.send_update(&row.session, update)
                .await
                .map_err(|_| fault::internal("the connection closed"))?;
        }
        if !self.roster.contains_key(&row.session) {
            self.host
                .resume(&row.session)
                .await
                .map_err(|why| {
                    tracing::warn!(?why, session = %row.session, "session/load: the host could not resume the helper's conversation");
                    fault::internal("the session could not be resumed")
                })?;
        }
        let mode = Mode::default();
        let helper = Some(row.id);
        self.roster.insert(row.session, Known { mode, helper });
        json(&LoadSessionResponse::new().modes(mode.state()))
    }

    /// The helpers this connection has a session for.
    pub(crate) fn watching(&self) -> Vec<HelperId> {
        let mut seen: Vec<HelperId> = Vec::new();
        for helper in self.roster.values().filter_map(|k| k.helper.as_ref()) {
            if !seen.contains(helper) {
                seen.push(helper.clone());
            }
        }
        seen
    }

    /// The board's sentence, as a plain agent line in each session this connection has for that
    /// helper.
    pub(crate) async fn tell(&mut self, told: ToldWire) -> Result<(), WireClosed> {
        let sessions: Vec<SessionId> = self
            .roster
            .iter()
            .filter(|(_, known)| known.helper.as_ref() == Some(&told.helper))
            .map(|(session, _)| session.clone())
            .collect();
        for session in sessions {
            self.send_update(&session, calls::say(told.text.clone()))
                .await?;
        }
        Ok(())
    }
}
