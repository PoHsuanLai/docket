//! `org.quire.IntentReach1`: which of a provider's actions are on right now. The installed manifest
//! says what an action is; a provider whose actions come and go with a setting (computer use is
//! off until the person turns it on) says so here, on the object it serves `IntentProvider1` at.
//! intentd asks the provider's own bus name, caches the answer, and asks again on `ReachChanged`.

use crate::names::PROVIDER_PATH;
use docket_manifest::AgentReach;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use zbus::fdo;
use zbus::object_server::SignalEmitter;

/// What a provider says about one action: the two values it may switch between.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReachNow {
    /// An agent may offer it.
    Offered,
    /// Only the person, through the app.
    Hidden,
}

impl From<ReachNow> for AgentReach {
    fn from(now: ReachNow) -> Self {
        match now {
            ReachNow::Offered => AgentReach::Offered,
            ReachNow::Hidden => AgentReach::Hidden,
        }
    }
}

/// The caller's side.
#[zbus::proxy(
    interface = "org.quire.IntentReach1",
    default_path = "/org/quire/IntentProvider1"
)]
pub trait IntentReach {
    /// `{ "<action name>": "offered" | "hidden" }` as JSON.
    fn reach(&self) -> zbus::Result<String>;

    /// The answer of `Reach` changed.
    #[zbus(signal)]
    fn reach_changed(&self) -> zbus::Result<()>;
}

/// Whether [`ReachServer::replace`] changed what is said.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Replaced {
    /// The same answer as before: nothing to announce.
    Same,
    /// A different answer: announce it.
    Different,
}

/// The provider's side: holds the answer and serves it. Clone it freely; the clones share one
/// answer. Serve it with [`ReachServer::serve`], change it with [`ReachServer::replace`] and
/// say so with [`ReachServer::announce`].
#[derive(Debug, Clone, Default)]
pub struct ReachServer {
    now: Arc<Mutex<BTreeMap<String, ReachNow>>>,
}

impl ReachServer {
    /// A server that says nothing about any action (every action keeps its manifest's reach).
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets what is said about each action named.
    pub fn replace(&self, next: impl IntoIterator<Item = (String, ReachNow)>) -> Replaced {
        let next: BTreeMap<String, ReachNow> = next.into_iter().collect();
        let mut held = self
            .now
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if *held == next {
            return Replaced::Same;
        }
        *held = next;
        Replaced::Different
    }

    /// The answer of `Reach` as it stands.
    pub fn json(&self) -> String {
        let held = self
            .now
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        serde_json::to_string(&*held).unwrap_or_else(|_| "{}".to_owned())
    }

    /// Serves `org.quire.IntentReach1` on `connection`, at the object `IntentProvider1` is at.
    pub async fn serve(&self, connection: &zbus::Connection) -> zbus::Result<()> {
        connection
            .object_server()
            .at(PROVIDER_PATH, self.clone())
            .await
            .map(|_| ())
    }

    /// Emits `ReachChanged` on `connection`.
    pub async fn announce(&self, connection: &zbus::Connection) -> zbus::Result<()> {
        let emitter = SignalEmitter::new(connection, PROVIDER_PATH)?;
        Self::reach_changed(&emitter).await
    }
}

#[zbus::interface(name = "org.quire.IntentReach1")]
impl ReachServer {
    fn reach(&self) -> fdo::Result<String> {
        Ok(self.json())
    }

    #[zbus(signal)]
    async fn reach_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}
