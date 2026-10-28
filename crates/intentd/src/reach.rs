//! Live reach: which actions a provider says are on right now (`org.quire.IntentReach1`). The
//! installed manifest is the app's declaration; a provider whose actions follow a setting may
//! switch its own actions between `hidden` and `offered`, nothing else. The answer is asked of
//! the owner of the app's own bus name (the identity check `Perform` uses), kept per app, and
//! laid over the manifest each time the manifests are loaded (`LiveReach::overlay`), so the
//! registry, the manifests clients read and the planners' menus all see one reach. It is asked
//! again when the app's name appears or vanishes and when any provider says `ReachChanged`; a
//! stranger's signal only makes intentd ask again, never decides anything.

use crate::builtin::is_builtin;
use crate::link::owned_by_us;
use crate::signals::rescan_with;
use docket_dbus::{BusConnection, IntentReachProxy, ReachNow};
use docket_manifest::{AgentReach, Manifest, ValidManifest};
use docket_router::{Router, Seams};
use futures_util::StreamExt;
use futures_util::future::join_all;
use porter_core::AppName;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::names::BusName;
use zbus::{MatchRule, MessageStream};

/// How long a provider may take to say its reach. A slow one keeps the manifest's value.
const ASK: Duration = Duration::from_secs(2);

/// What each provider on the bus last said, by app and action name.
#[derive(Debug, Clone, Default)]
pub struct LiveReach {
    said: Arc<Mutex<BTreeMap<AppName, BTreeMap<String, ReachNow>>>>,
}

/// The actions a provider's answer names, with the values this daemon understands: anything
/// else in the answer (an unknown value, a malformed document) is left out, so the manifest's
/// value stands.
pub fn parse_reach(text: &str) -> BTreeMap<String, ReachNow> {
    serde_json::from_str::<BTreeMap<String, serde_json::Value>>(text)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(action, value)| Some((action, serde_json::from_value(value).ok()?)))
        .collect()
}

impl LiveReach {
    /// Nothing said by anyone: every manifest is as its file declares.
    pub fn none() -> Self {
        Self::default()
    }

    /// Keeps what `app` said, replacing what it said before.
    pub fn keep(&self, app: AppName, said: BTreeMap<String, ReachNow>) {
        let mut held = self
            .said
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if said.is_empty() {
            held.remove(&app);
        } else {
            held.insert(app, said);
        }
    }

    /// `manifest` with what its app said laid over it. Only the actions the manifest declares
    /// change, and only between `hidden` and `offered`: an action the file asks about every time
    /// stays as it is.
    pub fn overlay(&self, manifest: ValidManifest) -> ValidManifest {
        let held = self
            .said
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let Some(said) = held.get(&manifest.manifest().app) else {
            return manifest;
        };
        let mut edited: Manifest = manifest.clone().into();
        for action in &mut edited.actions {
            let switchable = matches!(action.reach, AgentReach::Hidden | AgentReach::Offered);
            if let (true, Some(now)) = (switchable, said.get(action.name.as_str())) {
                action.reach = (*now).into();
            }
        }
        ValidManifest::try_from(edited).unwrap_or(manifest)
    }
}

/// The apps that are installed from files: the ones a provider may speak for.
fn installed<S: Seams>(router: &Router<S>) -> Vec<AppName> {
    let st = router
        .state
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    st.registry
        .all()
        .map(|m| m.manifest().app.clone())
        .filter(|app| !is_builtin(app))
        .collect()
}

/// What `app` says now: nothing when its name has no owner (it is never started for this), the
/// owner is not the person's own process, or it does not answer in time.
async fn ask(
    connection: &BusConnection,
    dbus: &DBusProxy<'_>,
    app: &AppName,
) -> BTreeMap<String, ReachNow> {
    let Ok(name) = BusName::try_from(app.to_string()) else {
        return BTreeMap::new();
    };
    if !dbus.name_has_owner(name.clone()).await.unwrap_or(false)
        || owned_by_us(dbus, &name).await.is_err()
    {
        return BTreeMap::new();
    }
    let Ok(builder) = IntentReachProxy::builder(connection).destination(app.to_string()) else {
        return BTreeMap::new();
    };
    let Ok(proxy) = builder.build().await else {
        return BTreeMap::new();
    };
    match tokio::time::timeout(ASK, proxy.reach()).await {
        Ok(Ok(text)) => parse_reach(&text),
        _ => BTreeMap::new(),
    }
}

/// Asks each of `apps`, keeps the answers, and loads the manifests again with them laid over.
async fn refresh<S: Seams>(
    router: &Router<S>,
    connection: &BusConnection,
    dbus: &DBusProxy<'_>,
    data_dirs: &[PathBuf],
    live: &LiveReach,
    apps: Vec<AppName>,
) {
    let answers = join_all(apps.into_iter().map(|app| async move {
        let said = ask(connection, dbus, &app).await;
        (app, said)
    }))
    .await;
    for (app, said) in answers {
        live.keep(app, said);
    }
    rescan_with(router, data_dirs, live);
}

/// Follows the providers' reach on `connection` until the task is dropped: asks every installed
/// app at the start, one app when its name appears or vanishes, all of them on `ReachChanged`.
pub async fn follow<S: Seams>(
    router: Arc<Router<S>>,
    connection: BusConnection,
    data_dirs: Vec<PathBuf>,
    live: LiveReach,
) {
    let Ok(dbus) = DBusProxy::new(&connection).await else {
        return;
    };
    let Ok(rule) = MatchRule::builder()
        .msg_type(Type::Signal)
        .interface("org.quire.IntentReach1")
        .and_then(|b| b.member("ReachChanged"))
        .map(|b| b.build())
    else {
        return;
    };
    let Ok(mut changed) = MessageStream::for_match_rule(rule, &connection, Some(64)).await else {
        return;
    };
    let Ok(mut owners) = dbus.receive_name_owner_changed().await else {
        return;
    };
    let all = installed(&router);
    refresh(&router, &connection, &dbus, &data_dirs, &live, all).await;
    loop {
        let apps = tokio::select! {
            signal = changed.next() => signal.map(|_| installed(&router)),
            owner = owners.next() => owner.map(|o| {
                let named = o.args().ok().map(|a| a.name().to_string());
                let named = |app: &AppName| Some(app.to_string()) == named;
                installed(&router).into_iter().filter(named).collect::<Vec<_>>()
            }),
        };
        let Some(apps) = apps else {
            return;
        };
        if !apps.is_empty() {
            refresh(&router, &connection, &dbus, &data_dirs, &live, apps).await;
        }
    }
}
