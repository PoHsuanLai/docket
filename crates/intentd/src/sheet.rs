//! The sheet that asks the person: `org.quire.Confirm1`, served by sill.

use docket_core::{ConfirmAnswer, ConfirmId, ConfirmRequest, Confirmer};

/// Asks through sill's `Confirm1` (a sheet in phase A, the trusted surface in phase B). The
/// receipt's input proof says which; a spoken answer is never one.
#[derive(Debug, Clone)]
pub struct SheetConfirmer {
    connection: docket_dbus::BusConnection,
}

impl SheetConfirmer {
    /// Asks over `connection`.
    pub fn new(connection: docket_dbus::BusConnection) -> Self {
        Self { connection }
    }
}

impl Confirmer for SheetConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        let _ = (&self.connection, request);
        todo!(
            "SheetConfirmer::confirm: Confirm1.Confirm, wait for the Request's Response; a vanished sill is Ended(Dismissed), never an allow"
        )
    }

    async fn cancel(&self, id: &ConfirmId) {
        let _ = (&self.connection, id);
        todo!("SheetConfirmer::cancel: Confirm1.Cancel")
    }
}
