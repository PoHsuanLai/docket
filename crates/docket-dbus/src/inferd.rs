//! The inferd link every daemon of this repo makes the same way.

use crate::tap::{Tap, Tapped};
use porter_client::{AnyTransport, DbusTransport};

/// What every daemon holds as its inferd link: porter-client's transport behind the tap that a
/// live run turns on (`DOCKET_MODEL_TRACE`, see [`crate::tap`]); with the tap off it is the
/// transport and nothing else.
pub type InferLink = Tapped<AnyTransport>;

/// inferd over the session bus: porter-client's D-Bus transport on the daemon's own
/// `connection`. Nothing is called here: inferd is found, and started by activation, at the
/// first `open`, so a daemon that starts before inferd still starts and, while inferd is away,
/// asks the person instead of failing. intentd, companiond and readerd all build their link
/// through this one function.
pub fn inferd_transport(connection: &crate::BusConnection) -> InferLink {
    Tapped::new(
        AnyTransport::Dbus(DbusTransport::over(connection.clone())),
        Tap::from_env(),
    )
}
