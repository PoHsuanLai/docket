//! What the routines need of the hub: where a helper works, what its jobs came to, the sentences
//! the person is told, and the records and events of the Spaces.

use super::owner::Hub;
use almanac_core::{RecentEntry, RecentQuery};
use companion_core::job_end;
use docket_client::Transport as IntentsTransport;
use docket_core::SessionOpen;
use docket_core::helper::{HelperName, HelperRecord, JobId};
use docket_core::routine::JobEnd;
use docket_planner::Planner;
use docket_tasks::{RecentSource, RouterRecent, Surface};
use docket_vocab::Clock;
use prov::{AgentRef, HelperId, SpaceId};

impl<P, I, K, S> Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// The Space of a helper.
    pub(crate) fn helper_space(&self, helper: &HelperId) -> Option<SpaceId> {
        self.with_state(|state| state.board.card(helper).map(|c| c.space.clone()))
    }

    /// What the person calls a helper.
    pub(crate) fn helper_name(&self, helper: &HelperId) -> Option<HelperName> {
        self.with_state(|state| state.board.card(helper).map(|c| c.name.clone()))
    }

    /// How a job ended, once it has: done, or open again after a failure. A job the board no
    /// longer holds did not finish.
    pub(crate) fn job_end(&self, job: JobId) -> Option<JobEnd> {
        self.with_state(|state| job_end(&state.board, job))
    }

    /// Tells the person a sentence about a helper, once, and the shell that something changed.
    pub(crate) fn say(&self, helper: &HelperId, text: String) {
        self.tell(helper, text);
        self.changed();
    }

    /// Writes `records` of the Space `space`.
    pub(crate) async fn write_records(&self, space: &SpaceId, records: &[HelperRecord]) {
        self.inner
            .service
            .lock()
            .await
            .write_helper_records(space, records)
            .await;
    }

    /// The routines' records the Spaces hold.
    pub(crate) async fn read_routine_records(&self, spaces: &[SpaceId]) -> Vec<HelperRecord> {
        self.inner
            .service
            .lock()
            .await
            .read_routine_records(spaces)
            .await
    }

    /// The events `query` asks for in `space`, newest first, through a session opened there for
    /// the purpose. A router that does not answer gives none.
    pub(crate) async fn recent_in(&self, space: &SpaceId, query: RecentQuery) -> Vec<RecentEntry> {
        let service = self.inner.service.lock().await;
        let opened = service
            .intents
            .session_open(SessionOpen {
                space: space.clone(),
                agent: AgentRef::Companion,
                parent: None,
                cwd: None,
                started_from: None,
                external: None,
                helper: None,
                acts_for: None,
            })
            .await;
        let Ok(opened) = opened else {
            tracing::warn!(?space, "no session to read events through");
            return Vec::new();
        };
        let found = RouterRecent::new(&service.intents, opened.session.clone())
            .recent(query)
            .await;
        let closed = service.intents.session_close(opened.session.clone()).await;
        if let Err(cause) = closed {
            tracing::warn!(session = %opened.session, %cause, "a session would not close");
        }
        found.unwrap_or_default()
    }
}
