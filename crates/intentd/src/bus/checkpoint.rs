//! `Checkpoint`: a session's restore points and what restoring one changes.

use super::{Gateway, json, parsed};
use docket_core::{CheckpointId, IntentsReply, IntentsRequest, WatchedAgent};
use docket_dbus::IntentsError;
use docket_vocab::{TurnId, Workspace};
use prov::SessionId;
use zbus::message::Header;

pub(crate) struct CheckpointBus(pub(crate) Gateway);

#[zbus::interface(name = "org.quire.Intents1.Checkpoint")]
impl CheckpointBus {
    async fn list(
        &self,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::CheckpointList {
            session: parsed(&session, SessionId::parse)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::Checkpoints(list) => Some(*list),
                _ => None,
            })
            .await
    }

    async fn plan(
        &self,
        session: String,
        point: u32,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::CheckpointPlan {
            session: parsed(&session, SessionId::parse)?,
            id: CheckpointId(point),
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::CheckpointPlan(plan) => Some(plan),
                _ => None,
            })
            .await
    }

    async fn watch(
        &self,
        workspace: String,
        label: String,
        rewind: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::CheckpointWatch {
            workspace: parsed(&workspace, Workspace::parse)?,
            label,
            rewind: json(&rewind)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::CheckpointWatching(session) => Some(session),
                _ => None,
            })
            .await
    }

    async fn mark(
        &self,
        session: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<u64, IntentsError> {
        let request = IntentsRequest::CheckpointMark {
            session: parsed(&session, SessionId::parse)?,
        };
        self.0
            .ask(&header, request, |r| match r {
                IntentsReply::TurnRecorded(TurnId(id)) => Some(id),
                _ => None,
            })
            .await
    }

    async fn watch_agent(
        &self,
        workspace: String,
        label: String,
        rewind: String,
        agent: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let request = IntentsRequest::CheckpointWatchAgent {
            workspace: parsed(&workspace, Workspace::parse)?,
            label,
            rewind: json(&rewind)?,
            agent: json(&agent)?,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::CheckpointWatchingAgent(watching) => Some(watching),
                _ => None,
            })
            .await
    }

    async fn handover(
        &self,
        session: String,
        to: String,
        agent: String,
        #[zbus(header)] header: Header<'_>,
    ) -> Result<String, IntentsError> {
        let agent: Option<WatchedAgent> = match agent.is_empty() {
            true => None,
            false => json(&agent)?,
        };
        let request = IntentsRequest::CheckpointHandover {
            session: parsed(&session, SessionId::parse)?,
            to: json(&to)?,
            agent,
        };
        self.0
            .answer(&header, request, |r| match r {
                IntentsReply::HandedOver(agent) => Some(agent),
                _ => None,
            })
            .await
    }

    async fn watched(&self, #[zbus(header)] header: Header<'_>) -> Result<String, IntentsError> {
        self.0
            .answer(&header, IntentsRequest::CheckpointWatched, |r| match r {
                IntentsReply::Watched(lines) => Some(lines),
                _ => None,
            })
            .await
    }
}
