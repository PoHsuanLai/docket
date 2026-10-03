//! The signals of `org.quire.Intents1` that say something changed inside the router: a manifest,
//! the undo journal, a session the breaker paused, a message that arrived. The router is pure
//! and answers requests; it does not push. Each signal is therefore the difference between two
//! looks at its state (`Marks`), taken a moment apart by the daemon (`pump`), so every way the
//! state can change (a call finishing, a message sent by the hosted Companion provider, a rescan
//! of the installed manifests) says so the same way.
//!
//! A signal is content-free: an app name, a number of rows, a session id, an agent. The first
//! look is the starting point and says nothing.

use crate::builtin::{builtin_manifests, is_builtin};
use crate::manifests::load_manifests;
use docket_core::ValidManifest;
use docket_dbus::{BusConnection, INTENTS_PATH};
use docket_router::{Router, Seams, SessionState};
use porter_core::AppName;
use prov::{AgentRef, MessageId, SessionId};
use std::collections::{BTreeMap, BTreeSet};
use std::hash::{Hash, Hasher};
use std::path::PathBuf;
use std::time::Duration;

/// What the router's state looked like at one moment, as little of it as the signals need.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Marks {
    /// A digest of every row of the undo journal and how many rows there are.
    journal: (u64, u64),
    /// The sessions the breaker has paused.
    paused: BTreeSet<SessionId>,
    /// The messages waiting for each agent.
    waiting: BTreeSet<(AgentRef, MessageId)>,
    /// A digest of each installed manifest.
    manifests: BTreeMap<AppName, u64>,
}

/// Something that changed, as the bus says it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Changed {
    /// An app's manifest was installed, replaced or removed (`Registry.ManifestChanged`).
    Manifest(AppName),
    /// The journal changed; this many rows now (`Control.JournalChanged`).
    Journal(u64),
    /// The breaker paused this session (`Control.BreakerTripped`).
    Breaker(SessionId),
    /// A message arrived for this agent (`Message.Arrived`).
    Arrived(AgentRef),
}

fn digest<T: serde::Serialize>(value: &T) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    serde_json::to_string(value)
        .unwrap_or_default()
        .hash(&mut hasher);
    hasher.finish()
}

/// The marks of `router` now.
pub fn marks_of<S: Seams>(router: &Router<S>) -> Marks {
    let st = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let rows = st.journal.entries();
    let waiting = st
        .sessions
        .values()
        .flat_map(|r| {
            let to = AgentRef::of(&r.actor);
            r.inbox
                .iter()
                .filter_map(move |m| Some((to.clone()?, m.id.clone())))
        })
        .chain(
            st.inboxes
                .iter()
                .flat_map(|(to, held)| held.iter().map(|m| (to.agent.clone(), m.id.clone()))),
        )
        .collect();
    Marks {
        journal: (
            digest(&rows.iter().map(|e| (e.id, &e.state)).collect::<Vec<_>>()),
            rows.len() as u64,
        ),
        paused: st
            .sessions
            .iter()
            .filter(|(_, r)| matches!(r.state, SessionState::Paused { .. }))
            .map(|(id, _)| id.clone())
            .collect(),
        waiting,
        manifests: st
            .registry
            .all()
            .map(|m| (m.manifest().app.clone(), digest(m.manifest())))
            .collect(),
    }
}

/// What changed between two looks, in a fixed order: manifests, the journal, the breaker, then
/// arrivals. A thing that was there before and is there still says nothing.
pub fn changes(before: &Marks, after: &Marks) -> Vec<Changed> {
    let apps: BTreeSet<&AppName> = before
        .manifests
        .keys()
        .chain(after.manifests.keys())
        .collect();
    let manifests = apps
        .into_iter()
        .filter(|app| before.manifests.get(*app) != after.manifests.get(*app))
        .map(|app| Changed::Manifest(app.clone()));
    let journal = (before.journal != after.journal).then_some(Changed::Journal(after.journal.1));
    let paused = after
        .paused
        .difference(&before.paused)
        .map(|s| Changed::Breaker(s.clone()));
    let agents: BTreeSet<&AgentRef> = after
        .waiting
        .difference(&before.waiting)
        .map(|(agent, _)| agent)
        .collect();
    let arrived = agents.into_iter().map(|a| Changed::Arrived(a.clone()));
    manifests
        .chain(journal)
        .chain(paused)
        .chain(arrived)
        .collect()
}

/// Says one change on the bus, to everyone who listens.
pub async fn emit(connection: &BusConnection, change: &Changed) -> zbus::Result<()> {
    let (interface, member, body) = match change {
        Changed::Manifest(app) => (
            "org.quire.Intents1.Registry",
            "ManifestChanged",
            serde_json::Value::String(app.to_string()),
        ),
        Changed::Journal(rows) => (
            "org.quire.Intents1.Control",
            "JournalChanged",
            serde_json::Value::from(*rows),
        ),
        Changed::Breaker(session) => (
            "org.quire.Intents1.Control",
            "BreakerTripped",
            serde_json::Value::String(session.to_string()),
        ),
        Changed::Arrived(agent) => (
            "org.quire.Intents1.Message",
            "Arrived",
            serde_json::Value::String(serde_json::to_string(agent).unwrap_or_default()),
        ),
    };
    let none = None::<zbus::names::BusName<'_>>;
    match body {
        serde_json::Value::String(text) => {
            connection
                .emit_signal(none, INTENTS_PATH, interface, member, &text)
                .await
        }
        serde_json::Value::Number(n) => {
            connection
                .emit_signal(
                    none,
                    INTENTS_PATH,
                    interface,
                    member,
                    &n.as_u64().unwrap_or(0),
                )
                .await
        }
        _ => Ok(()),
    }
}

/// Makes the registry what the installed manifests say, beside the built-in ones: a new or
/// changed file is installed, a file that is gone takes its app with it. The built-in providers
/// are never replaced or removed by a file.
pub fn rescan<S: Seams>(router: &Router<S>, data_dirs: &[PathBuf]) {
    let loaded = load_manifests(data_dirs).manifests;
    let wanted: BTreeMap<AppName, ValidManifest> = loaded
        .into_iter()
        .map(|m| (m.manifest().app.clone(), m))
        .collect();
    let mut st = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let gone: Vec<AppName> = st
        .registry
        .all()
        .map(|m| m.manifest().app.clone())
        .filter(|app| !is_builtin(app) && !wanted.contains_key(app))
        .collect();
    for app in gone {
        st.registry.remove(&app);
    }
    for (app, manifest) in wanted {
        if st.registry.get(&app) != Some(&manifest) {
            st.registry.insert(manifest);
        }
    }
    // The built-ins are always there.
    for manifest in builtin_manifests().unwrap_or_default() {
        if st.registry.get(&manifest.manifest().app).is_none() {
            st.registry.insert(manifest);
        }
    }
}

/// How often the daemon looks, and how many looks between rescans of the manifest directories.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cadence {
    /// Between two looks at the router's state.
    pub every: Duration,
    /// Looks between two rescans of the installed manifests.
    pub rescan_every: u32,
}

impl Default for Cadence {
    fn default() -> Self {
        Self {
            every: Duration::from_millis(200),
            rescan_every: 15,
        }
    }
}

/// Looks at the router every `cadence.every`, rescans the manifests every few looks, and says
/// each change on `connection`. Runs until the task is aborted.
pub async fn pump<S: Seams>(
    router: std::sync::Arc<Router<S>>,
    connection: BusConnection,
    data_dirs: Vec<PathBuf>,
    cadence: Cadence,
) {
    let mut last = marks_of(&router);
    let mut tick = 0u32;
    loop {
        tokio::time::sleep(cadence.every).await;
        tick = tick.wrapping_add(1);
        if cadence.rescan_every > 0 && tick.is_multiple_of(cadence.rescan_every) {
            rescan(&router, &data_dirs);
        }
        let now = marks_of(&router);
        for change in changes(&last, &now) {
            let _ = emit(&connection, &change).await;
        }
        last = now;
    }
}
