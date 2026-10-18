//! The helpers and their routines, as a seam: list, read a card, make, change, pause, resume and
//! remove a helper, give it a job, sign in to the app it runs, and the same for its routines.
//! [`HelpersTransport`] is what a face of the helpers is written against; [`DbusHelpers`] is
//! `org.quire.Companion1.Helpers2` on the session bus. Only the shell may use it; the daemon
//! refuses anyone else.

use crate::bus::{fault_of, json};
use companion_wire::{
    HelperCardView, HelperForm, HelperReply, HelpersView, ProposalId, RoutineCardView,
    RoutineForm, RoutineProposal, RoutineReply, RoutinesView,
};
use docket_client::TransportError;
use docket_core::routine::RoutineId;
use docket_dbus::{BusConnection, CompanionHelpers2Proxy};
use futures_util::{Stream, StreamExt};
use prov::{EntityId, HelperId};
use serde::de::DeserializeOwned;
use std::future::Future;
use std::pin::Pin;

/// Each time something about the helpers or their routines changed. The signal carries nothing:
/// read the list again.
pub type Changes = Pin<Box<dyn Stream<Item = ()> + Send>>;

/// One carrier of the helpers' calls.
pub trait HelpersTransport: Send + Sync {
    /// The helpers, and what they told the person after the sentence numbered `after`.
    fn list(&self, after: u64) -> impl Future<Output = Result<HelpersView, TransportError>> + Send;

    /// One helper's card.
    fn card(
        &self,
        helper: &HelperId,
    ) -> impl Future<Output = Result<HelperCardView, TransportError>> + Send;

    /// Makes a helper from what the person confirmed.
    fn create(
        &self,
        form: &HelperForm,
    ) -> impl Future<Output = Result<HelperReply, TransportError>> + Send;

    /// Changes a helper to what the person confirmed.
    fn edit(
        &self,
        helper: &HelperId,
        form: &HelperForm,
    ) -> impl Future<Output = Result<HelperReply, TransportError>> + Send;

    /// The person pauses a helper.
    fn pause(&self, helper: &HelperId) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The person resumes a helper, or asks one whose job did not finish to try again.
    fn resume(&self, helper: &HelperId) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The person removes a helper and its jobs.
    fn remove(&self, helper: &HelperId) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The person gives a helper a job; `things` are the only things that job may change.
    fn post_job(
        &self,
        helper: &HelperId,
        goal: &str,
        things: &[EntityId],
    ) -> impl Future<Output = Result<HelperReply, TransportError>> + Send;

    /// The person chose one of the ways of signing in to the app the helper runs.
    fn sign_in(
        &self,
        helper: &HelperId,
        way: &str,
    ) -> impl Future<Output = Result<HelperReply, TransportError>> + Send;

    /// The person signs out of the app the helper runs.
    fn sign_out(
        &self,
        helper: &HelperId,
    ) -> impl Future<Output = Result<HelperReply, TransportError>> + Send;

    /// The routines.
    fn routines(&self) -> impl Future<Output = Result<RoutinesView, TransportError>> + Send;

    /// One routine with its recent runs.
    fn routine_card(
        &self,
        routine: RoutineId,
    ) -> impl Future<Output = Result<RoutineCardView, TransportError>> + Send;

    /// Makes a routine from what the person confirmed.
    fn add_routine(
        &self,
        form: &RoutineForm,
    ) -> impl Future<Output = Result<RoutineReply, TransportError>> + Send;

    /// The person pauses a routine.
    fn pause_routine(
        &self,
        routine: RoutineId,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The person resumes a routine.
    fn resume_routine(
        &self,
        routine: RoutineId,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The person removes a routine.
    fn remove_routine(
        &self,
        routine: RoutineId,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// The routines proposed that wait for the person.
    fn routine_proposals(
        &self,
    ) -> impl Future<Output = Result<Vec<RoutineProposal>, TransportError>> + Send;

    /// The person accepts a proposal: the routine is made.
    fn accept_routine_proposal(
        &self,
        id: ProposalId,
    ) -> impl Future<Output = Result<RoutineReply, TransportError>> + Send;

    /// The person declines a proposal.
    fn decline_routine_proposal(
        &self,
        id: ProposalId,
    ) -> impl Future<Output = Result<(), TransportError>> + Send;

    /// A stream of the times something changed, subscribed from now.
    fn changes(&self) -> impl Future<Output = Result<Changes, TransportError>> + Send;
}

/// companiond's helpers over D-Bus, as the connection's caller.
#[derive(Debug)]
pub struct DbusHelpers {
    helpers: CompanionHelpers2Proxy<'static>,
}

impl DbusHelpers {
    /// Talks to companiond over `connection`; the bus starts it on the first call when it is
    /// installed with an activation file.
    pub async fn over(connection: &BusConnection) -> Result<Self, TransportError> {
        let helpers = CompanionHelpers2Proxy::new(connection)
            .await
            .map_err(fault_of)?;
        Ok(Self { helpers })
    }

    /// Connects to the session bus.
    pub async fn connect() -> Result<Self, TransportError> {
        let connection = BusConnection::session()
            .await
            .map_err(|e| TransportError::Bus(e.to_string()))?;
        Self::over(&connection).await
    }
}

fn read<T: DeserializeOwned>(text: &str) -> Result<T, TransportError> {
    serde_json::from_str(text).map_err(|e| TransportError::Malformed(e.to_string()))
}

/// A helper's id as the bus carries it: the id's own text.
fn helper_text(helper: &HelperId) -> Result<String, TransportError> {
    match serde_json::to_value(helper) {
        Ok(serde_json::Value::String(text)) => Ok(text),
        _ => Err(TransportError::Malformed("a helper id".to_owned())),
    }
}

impl HelpersTransport for DbusHelpers {
    async fn list(&self, after: u64) -> Result<HelpersView, TransportError> {
        read(&self.helpers.list(after).await.map_err(fault_of)?)
    }

    async fn card(&self, helper: &HelperId) -> Result<HelperCardView, TransportError> {
        read(
            &self
                .helpers
                .card(&helper_text(helper)?)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn create(&self, form: &HelperForm) -> Result<HelperReply, TransportError> {
        read(&self.helpers.create(&json(form)?).await.map_err(fault_of)?)
    }

    async fn edit(
        &self,
        helper: &HelperId,
        form: &HelperForm,
    ) -> Result<HelperReply, TransportError> {
        read(
            &self
                .helpers
                .edit(&helper_text(helper)?, &json(form)?)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn pause(&self, helper: &HelperId) -> Result<(), TransportError> {
        self.helpers
            .pause(&helper_text(helper)?)
            .await
            .map_err(fault_of)
    }

    async fn resume(&self, helper: &HelperId) -> Result<(), TransportError> {
        self.helpers
            .resume(&helper_text(helper)?)
            .await
            .map_err(fault_of)
    }

    async fn remove(&self, helper: &HelperId) -> Result<(), TransportError> {
        self.helpers
            .remove(&helper_text(helper)?)
            .await
            .map_err(fault_of)
    }

    async fn post_job(
        &self,
        helper: &HelperId,
        goal: &str,
        things: &[EntityId],
    ) -> Result<HelperReply, TransportError> {
        read(
            &self
                .helpers
                .post_job(&helper_text(helper)?, goal, &json(&things)?)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn sign_in(&self, helper: &HelperId, way: &str) -> Result<HelperReply, TransportError> {
        read(
            &self
                .helpers
                .sign_in(&helper_text(helper)?, way)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn sign_out(&self, helper: &HelperId) -> Result<HelperReply, TransportError> {
        read(
            &self
                .helpers
                .sign_out(&helper_text(helper)?)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn routines(&self) -> Result<RoutinesView, TransportError> {
        read(&self.helpers.routines().await.map_err(fault_of)?)
    }

    async fn routine_card(&self, routine: RoutineId) -> Result<RoutineCardView, TransportError> {
        read(
            &self
                .helpers
                .routine_card(&routine.to_string())
                .await
                .map_err(fault_of)?,
        )
    }

    async fn add_routine(&self, form: &RoutineForm) -> Result<RoutineReply, TransportError> {
        read(
            &self
                .helpers
                .add_routine(&json(form)?)
                .await
                .map_err(fault_of)?,
        )
    }

    async fn pause_routine(&self, routine: RoutineId) -> Result<(), TransportError> {
        self.helpers
            .pause_routine(&routine.to_string())
            .await
            .map_err(fault_of)
    }

    async fn resume_routine(&self, routine: RoutineId) -> Result<(), TransportError> {
        self.helpers
            .resume_routine(&routine.to_string())
            .await
            .map_err(fault_of)
    }

    async fn remove_routine(&self, routine: RoutineId) -> Result<(), TransportError> {
        self.helpers
            .remove_routine(&routine.to_string())
            .await
            .map_err(fault_of)
    }

    async fn routine_proposals(&self) -> Result<Vec<RoutineProposal>, TransportError> {
        read(&self.helpers.routine_proposals().await.map_err(fault_of)?)
    }

    async fn accept_routine_proposal(
        &self,
        id: ProposalId,
    ) -> Result<RoutineReply, TransportError> {
        read(
            &self
                .helpers
                .accept_routine_proposal(&id.to_string())
                .await
                .map_err(fault_of)?,
        )
    }

    async fn decline_routine_proposal(&self, id: ProposalId) -> Result<(), TransportError> {
        self.helpers
            .decline_routine_proposal(&id.to_string())
            .await
            .map_err(fault_of)
    }

    async fn changes(&self) -> Result<Changes, TransportError> {
        let signals = self.helpers.receive_changed().await.map_err(fault_of)?;
        Ok(Box::pin(signals.map(|_| ())))
    }
}
