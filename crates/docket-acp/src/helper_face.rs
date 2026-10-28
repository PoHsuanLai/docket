//! The editor's face of the helpers (design note `swarm.md`, section 7.2): which helpers an
//! editor sees in a workspace, opening and loading their sessions, and the board's sentences
//! streamed as plain agent lines. A helper's turns, calls and sheets are the ordinary ones: its
//! session is in the same host, and the person's words to it are the editor's, with the `Editor`
//! role's caps, like any other prompt. A helper's conversation is the exception: its turns run
//! with the helper (`ConversationTurns`).

use crate::calls;
use crate::helper_replay;
use crate::helpers::{HelperRow, Helpers};
use crate::link::{Gone, Link};
use crate::mode::Mode;
use crate::out;
use crate::server::{Known, Server, Turns};
use acp_core::fault;
use agent_client_protocol::schema::v1::{
    Error, LoadSessionResponse, Meta, NewSessionResponse, SessionInfo,
};
use companion_wire::ToldWire;
use docket_session::{SessionHost, SessionLog};
use docket_vocab::{Clock, Workspace};
use prov::{HelperId, SessionId, SpaceId};

/// The Space a workspace maps to (owner decision D4). Every workspace maps to the desktop Space
/// until the per-workspace setting exists; sessions an editor opens and the helpers it lists use
/// this one function, so they cannot disagree.
pub(crate) fn space_of(_workspace: &Workspace) -> SpaceId {
    SpaceId::desktop()
}

/// The helper a `session/new` names, in its `_meta`.
pub(crate) fn helper_asked(meta: Option<&Meta>) -> Option<String> {
    meta.and_then(|meta| meta.get("helper"))
        .and_then(serde_json::Value::as_str)
        .map(|name| name.trim().to_owned())
}

impl<H: SessionHost, L: SessionLog, T: Clock, S: Helpers> Server<H, L, T, S> {
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

    /// `session/new` with `_meta.helper`: a job session for the helper called `name`.
    pub(crate) async fn new_helper_job(
        &mut self,
        name: &str,
        workspace: &Workspace,
    ) -> Result<NewSessionResponse, Error> {
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
        let turns = Turns::Here;
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
    pub(crate) async fn load_helper(
        &mut self,
        link: &Link,
        row: HelperRow,
    ) -> Result<LoadSessionResponse, Error> {
        let jobs = self.helpers.jobs(&row.id);
        for update in helper_replay::recent(&row.name, &jobs) {
            link.update(&row.session, update)
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
        let turns = Turns::AtHelper;
        self.roster.insert(
            row.session,
            Known {
                mode,
                helper,
                turns,
            },
        );
        Ok(LoadSessionResponse::new().modes(mode.state()))
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
    pub(crate) fn tell(&mut self, link: &Link, told: ToldWire) -> Result<(), Gone> {
        let sessions: Vec<SessionId> = self
            .roster
            .iter()
            .filter(|(_, known)| known.helper.as_ref() == Some(&told.helper))
            .map(|(session, _)| session.clone())
            .collect();
        for session in sessions {
            link.update(&session, calls::say(told.text.clone()))?;
        }
        Ok(())
    }
}
