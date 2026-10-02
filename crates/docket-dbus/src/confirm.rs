//! `org.quire.Confirm1`: the sheet that asks the person, served by sill (a sheet in phase A, the trusted surface in phase B).

use zbus::fdo;
use zbus::zvariant::OwnedObjectPath;

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.Confirm1",
    default_service = "org.quire.Confirm1",
    default_path = "/org/quire/Confirm1"
)]
pub trait Confirm {
    /// Shows a confirmation (`ConfirmRequest` JSON). Answers a Request whose `Response` carries the `ConfirmAnswer`.
    fn confirm(&self, request: &str) -> zbus::Result<OwnedObjectPath>;

    /// Withdraws a pending confirmation.
    fn cancel(&self, id: &str) -> zbus::Result<()>;
}

/// The daemon's side.
#[derive(Debug, Default)]
pub struct ConfirmSkeleton;

#[zbus::interface(name = "org.quire.Confirm1")]
impl ConfirmSkeleton {
    fn confirm(&self, request: String) -> fdo::Result<OwnedObjectPath> {
        let _ = (request,);
        Err(crate::introspect::frozen())
    }

    fn cancel(&self, id: String) -> fdo::Result<()> {
        let _ = (id,);
        Err(crate::introspect::frozen())
    }
}
