//! docket's D-Bus API: `org.quire.Intents1` (the router, split into interfaces), the per-app
//! `org.quire.IntentProvider1`, sill's `org.quire.Confirm1`, `org.quire.Companion1` and
//! `org.quire.Reader1`. Each interface is declared twice from one table: a proxy trait for
//! callers and a skeleton for the daemons, whose introspection is the checked-in `dbus/*.xml`
//! (see `tests/it/introspection.rs`). Bodies are the JSON of the named `docket-core` type in an
//! `s` argument; the caller's identity is derived from the connection and never sent.
//! Signatures only: every skeleton method answers `NotSupported`.
//!
//! `org.quire.IntentReach1` (`reach`) rides on a provider's `IntentProvider1` object: which of its
//! actions are on now.
//!
//! `Voice1` lives in `voiced`, which serves it.

mod agent_host;
mod answer;
pub mod callers;
mod checkpoint;
mod companion;
mod confirm;
mod context;
mod control;
mod error;
mod gate;
mod helpers;
mod helpers2;
mod index;
#[cfg(feature = "inferd")]
mod inferd;
mod introspect;
mod message;
mod names;
mod provider;
mod reach;
mod reader;
mod registry;
mod request;
mod run;
mod search;
mod session;
mod terminals;
#[cfg(feature = "inferd")]
mod watch;

pub use agent_host::{AgentHostProxy, AgentHostSkeleton};
pub use answer::{CompanionAnswerProxy, CompanionAnswerSkeleton};
pub use checkpoint::{CheckpointProxy, CheckpointSkeleton};
pub use companion::{CompanionProxy, CompanionSkeleton};
pub use confirm::{ConfirmProxy, ConfirmSkeleton};
pub use context::{ContextProxy, ContextSkeleton};
pub use control::{ControlProxy, ControlSkeleton};
pub use error::IntentsError;
pub use gate::{GateProxy, GateSkeleton};
pub use helpers::{CompanionHelpersProxy, CompanionHelpersSkeleton};
pub use helpers2::{CompanionHelpers2Proxy, CompanionHelpers2Skeleton};
pub use index::{IndexProxy, IndexSkeleton};
#[cfg(feature = "inferd")]
pub use inferd::{InferLink, inferd_transport};
pub use introspect::{Bus, introspection};
pub use message::{MessageProxy, MessageSkeleton};
pub use names::{
    AGENT_HOST_BUS, AGENT_HOST_PATH, COMPANION_BUS, COMPANION_PATH, CONFIRM_BUS, CONFIRM_PATH,
    INTENTS_BUS, INTENTS_PATH, OPTION_ACTIVATION, OPTION_WATCH, PROVIDER_PATH, READER_BUS,
    READER_PATH, answer_path, request_path, session_connection,
};
/// The reserved key of the W3C trace context in an `options` vardict, and the vardict type:
/// porter's, shared on every call that starts work.
pub use porter_dbus::Details;
pub use provider::{IntentProviderProxy, IntentProviderSkeleton};
pub use reach::{IntentReachProxy, ReachNow, ReachServer};
pub use reader::{ReaderProxy, ReaderSkeleton};
pub use registry::{RegistryProxy, RegistrySkeleton};
pub use request::{RequestProxy, RequestSkeleton};
pub use run::{RunProxy, RunSkeleton};
pub use search::{SearchProxy, SearchSkeleton};
pub use session::{SessionProxy, SessionSkeleton};
pub use steward_dbus::inference::OPTION_TRACEPARENT;
pub use terminals::{CompanionTerminalsProxy, CompanionTerminalsSkeleton};
#[cfg(feature = "inferd")]
pub use watch::{InferObserver, InferWatch, SessionWatch, Watched, WatchedSession};
/// The session-bus connection transports and daemons hold.
pub use zbus::Connection as BusConnection;
