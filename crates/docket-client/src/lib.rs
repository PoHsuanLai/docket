//! docket's client API. An app implements [`IntentProvider`] and answers the router's calls;
//! a caller (sill, companiond, an MCP edge) holds [`Intents`] over a [`Transport`] and asks the
//! router. The transport is the only thing that changes between the desktop's D-Bus
//! (`DbusTransport`, feature `dbus`) and a test or an embedding host ([`InProcess`]).

mod intents;
mod provider;
mod serve;
mod transport;

pub use intents::{ClientError, Intents};
pub use provider::{ContextSource, IntentProvider, SummonTarget};
pub use serve::serve;
#[cfg(feature = "dbus")]
pub use transport::DbusTransport;
pub use transport::{InProcess, Transport, TransportError};
