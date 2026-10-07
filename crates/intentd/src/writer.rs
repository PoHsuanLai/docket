//! The policy writer over inferd. The writer itself is `docket-models`' `TransportWriter` (it
//! reads the person's own turns and the action catalogue, never content; nothing in a draft is
//! believed as written); this adds the constructor over the session bus.

use docket_core::{ActionCard, PolicyWriter, ReviewError, TaskPolicy, UserTurn};
use docket_dbus::InferLink;
use docket_models::TransportWriter;
use porter_client::Transport;
use prov::{SpaceId, TaskId};

/// The policy writer: reads the person's turns and the action catalogue, never content.
#[derive(Debug)]
pub struct InferdWriter<T: Transport>(TransportWriter<T>);

impl<T: Transport> InferdWriter<T> {
    /// Asks through `transport`.
    pub fn new(transport: T) -> Self {
        Self(TransportWriter::new(transport))
    }
}

impl InferdWriter<InferLink> {
    /// Asks inferd over the session bus.
    pub fn on_bus(connection: &docket_dbus::BusConnection) -> Self {
        Self::new(crate::infer::inferd_transport(connection))
    }
}

impl<T: Transport> PolicyWriter for InferdWriter<T> {
    async fn derive(
        &self,
        task: &TaskId,
        turns: &[UserTurn],
        catalogue: &[ActionCard],
        space: &SpaceId,
    ) -> Result<TaskPolicy, ReviewError> {
        self.0.derive(task, turns, catalogue, space).await
    }
}
