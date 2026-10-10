//! docket's client API. An app implements [`IntentProvider`] and answers the router's calls;
//! a caller (sill, companiond, an MCP edge) holds [`Intents`] over a [`Transport`] and asks the
//! router. The transport is the only thing that changes between the desktop's D-Bus
//! (`DbusTransport`, feature `dbus`) and a test or an embedding host (`InProcess`, feature
//! `in_process`, which is the only way this crate reaches the router, cedar and policy-point).

#[cfg(feature = "dbus")]
mod awaiting;
#[cfg(feature = "dbus")]
mod bus;
mod checkpoint_calls;
mod intents;
mod provider;
#[cfg(feature = "dbus")]
mod provider_bus;
mod serve;
mod session_calls;
mod transport;
mod watch;
#[cfg(feature = "dbus")]
mod watch_bus;
#[cfg(feature = "in_process")]
mod watch_in_process;

#[cfg(feature = "dbus")]
pub use awaiting::requested;
pub use intents::{ClientError, Intents};
pub use provider::{ContextSource, IntentProvider, SummonTarget};
#[cfg(feature = "dbus")]
pub use provider_bus::serve_on;
pub use serve::serve;
#[cfg(feature = "dbus")]
pub use transport::DbusTransport;
#[cfg(feature = "in_process")]
pub use transport::InProcess;
pub use transport::{Transport, TransportError};
pub use watch::{
    Boxed, Events, GateEvent, GateSteer, GateWatch, PerformEvent, PerformWatch, Said, Steering,
    Watched,
};
