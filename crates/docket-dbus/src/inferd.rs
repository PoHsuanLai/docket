//! The inferd link every daemon of this repo makes the same way.

use porter_client::{AnyTransport, DbusTransport};

/// inferd over the session bus: porter-client's D-Bus transport on the daemon's own
/// `connection`. Nothing is called here: inferd is found, and started by activation, at the
/// first `open`, so a daemon that starts before inferd still starts and, while inferd is away,
/// asks the person instead of failing. intentd, companiond and readerd all build their link
/// through this one function.
pub fn inferd_transport(connection: &crate::BusConnection) -> AnyTransport {
    AnyTransport::Dbus(DbusTransport::over(connection.clone()))
}
