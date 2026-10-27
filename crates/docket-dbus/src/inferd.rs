//! The inferd link every daemon of this repo makes the same way.

use crate::watch::{InferWatch, Watched};
use porter_client::{AnyTransport, DbusTransport};

/// What every daemon holds as its inferd link: porter-client's transport behind the watch that a
/// live run turns on (see [`crate::watch`]; only the acceptance harness's mains pass one that
/// keeps anything); with the watch off it is the transport and nothing else.
pub type InferLink = Watched<AnyTransport>;

/// inferd over the session bus: porter-client's D-Bus transport on the daemon's own
/// `connection`. Nothing is called here: inferd is found, and started by activation, at the
/// first `open`, so a daemon that starts before inferd still starts and, while inferd is away,
/// asks the person instead of failing. intentd, companiond and readerd all build their link
/// through this one function.
pub fn inferd_transport(connection: &crate::BusConnection, watch: InferWatch) -> InferLink {
    Watched::new(
        AnyTransport::Dbus(DbusTransport::over(connection.clone())),
        watch,
    )
}
