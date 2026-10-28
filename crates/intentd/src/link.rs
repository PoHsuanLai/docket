//! The apps, over D-Bus: `IntentProvider1` on each app's own bus name. The router calls it only
//! after checking that the name's owner is the person's own process.

use docket_core::LinkFault;
use docket_core::{
    AppRefusal, ContextScope, ContextSnapshot, FailText, Invocation, Latency, Outcome, Preview,
    SuggestAsk, UndoFault,
};
use docket_dbus::{BusConnection, Details, IntentProviderProxy};
use docket_procroot::own_uid;
use docket_router::{AppFault, AppLink};
use docket_vocab::{EntityRef, UndoToken};
use porter_core::AppName;
use prov::{Actor, EntityId};
use serde::Serialize;
use serde::de::DeserializeOwned;
use std::future::Future;
use std::time::Duration;
use zbus::fdo::DBusProxy;
use zbus::names::BusName;

/// How long an action that answers at once may take.
const INSTANT: Duration = Duration::from_millis(250);
/// How long an action that answers within seconds may take.
const QUICK: Duration = Duration::from_secs(5);
/// The most a long action may take before the router gives up on it; the app reports progress
/// to the person through its own window.
const LONG: Duration = Duration::from_secs(600);
/// How long starting an app that is not running may take.
const ACTIVATION: Duration = Duration::from_secs(10);
/// What a preview, a suggestion or a context may take.
const READ: Duration = Duration::from_secs(5);

/// Whether a call may start an app that is not running: an action may, a lookup never does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Wake {
    /// Start the app by bus activation if its name has no owner.
    Start,
    /// Ask only an app that is running; one that is not is unavailable.
    IfRunning,
}

/// `AppLink` over the session bus.
#[derive(Debug, Clone)]
pub struct DbusLink {
    connection: BusConnection,
}

fn budget(latency: Latency) -> Duration {
    match latency {
        Latency::Instant => INSTANT,
        Latency::Quick => QUICK,
        Latency::Long => LONG,
    }
}

fn json<T: Serialize>(value: &T) -> Result<String, LinkFault> {
    serde_json::to_string(value).map_err(|_| LinkFault::Malformed)
}

fn read<T: DeserializeOwned>(text: &str) -> Result<T, LinkFault> {
    serde_json::from_str(text).map_err(|_| LinkFault::Malformed)
}

/// A bus error that means the app is not there, or something else.
fn fault_of(error: &zbus::Error) -> LinkFault {
    match error {
        zbus::Error::MethodError(name, _, _)
            if matches!(
                name.as_str(),
                "org.freedesktop.DBus.Error.ServiceUnknown"
                    | "org.freedesktop.DBus.Error.NameHasNoOwner"
                    | "org.freedesktop.DBus.Error.Disconnected"
            ) =>
        {
            LinkFault::Unavailable
        }
        zbus::Error::InputOutput(_) => LinkFault::Unavailable,
        _ => LinkFault::Malformed,
    }
}

fn gone() -> AppRefusal {
    AppRefusal::Failed(FailText("the app is not available".into()))
}

/// Checks that the owner of `name` is the person's own process: the identity every call to a
/// provider (and every question about its live reach) is made on.
pub(crate) async fn owned_by_us(dbus: &DBusProxy<'_>, name: &BusName<'_>) -> Result<(), LinkFault> {
    let owner = dbus
        .get_name_owner(name.clone())
        .await
        .map_err(|_| LinkFault::Unavailable)?;
    let credentials = dbus
        .get_connection_credentials(BusName::Unique(owner.into()))
        .await
        .map_err(|_| LinkFault::Unavailable)?;
    match own_uid() {
        Some(ours) if credentials.unix_user_id() != Some(ours) => Err(LinkFault::Unavailable),
        _ => Ok(()),
    }
}

impl DbusLink {
    /// Calls apps over `connection`.
    pub fn new(connection: BusConnection) -> Self {
        Self { connection }
    }

    /// Starts the app if its name has no owner and `wake` allows it, and checks that the owner
    /// is the person's own process.
    async fn proxy(
        &self,
        app: &AppName,
        wake: Wake,
    ) -> Result<IntentProviderProxy<'static>, LinkFault> {
        let dbus = DBusProxy::new(&self.connection)
            .await
            .map_err(|_| LinkFault::Unavailable)?;
        let name = BusName::try_from(app.to_string()).map_err(|_| LinkFault::Malformed)?;
        let owned = dbus.name_has_owner(name.clone()).await.unwrap_or(false);
        if !owned && wake == Wake::IfRunning {
            return Err(LinkFault::Unavailable);
        }
        if !owned {
            let started = dbus.start_service_by_name(
                name.clone().try_into().map_err(|_| LinkFault::Malformed)?,
                0,
            );
            match tokio::time::timeout(ACTIVATION, started).await {
                Ok(Ok(_)) => {}
                _ => return Err(LinkFault::Unavailable),
            }
        }
        owned_by_us(&dbus, &name).await?;
        IntentProviderProxy::builder(&self.connection)
            .destination(app.to_string())
            .map_err(|_| LinkFault::Malformed)?
            .build()
            .await
            .map_err(|e| fault_of(&e))
    }

    /// One call to the app within `within`: the text it answered.
    async fn ask<F, Fut>(
        &self,
        app: &AppName,
        wake: Wake,
        within: Duration,
        call: F,
    ) -> Result<String, LinkFault>
    where
        F: FnOnce(IntentProviderProxy<'static>) -> Fut,
        Fut: Future<Output = zbus::Result<String>>,
    {
        let proxy = self.proxy(app, wake).await?;
        match tokio::time::timeout(within, call(proxy)).await {
            Ok(Ok(text)) => Ok(text),
            Ok(Err(error)) => Err(fault_of(&error)),
            Err(_) => Err(LinkFault::Timeout),
        }
    }
}

impl AppLink for DbusLink {
    async fn perform(
        &self,
        app: &AppName,
        inv: Invocation,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        self.perform_activated(app, inv, None, within).await
    }

    async fn perform_activated(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        self.perform_classified(app, inv, activation, None, within)
            .await
    }

    async fn classify(
        &self,
        app: &AppName,
        inv: Invocation,
    ) -> Result<docket_vocab::CallClass, docket_vocab::ClassifyFault> {
        use docket_vocab::ClassifyFault as Fault;
        let text = json(&inv).map_err(|_| Fault::Unavailable)?;
        let answered = self
            .ask(app, Wake::Start, READ, |p| async move {
                p.classify(&text, &Details::new()).await
            })
            .await;
        match answered {
            Ok(text) => read::<Result<docket_vocab::CallClass, AppRefusal>>(&text)
                .map_err(|_| Fault::Unavailable)?
                .map_err(|_| Fault::Refused),
            Err(LinkFault::Timeout) => Err(Fault::TimedOut),
            Err(LinkFault::Unavailable | LinkFault::Malformed) => Err(Fault::Unavailable),
        }
    }

    async fn perform_classified(
        &self,
        app: &AppName,
        inv: Invocation,
        activation: Option<docket_core::ActivationToken>,
        classified: Option<docket_vocab::CallClass>,
        within: Latency,
    ) -> Result<Outcome, AppFault> {
        let delivered = docket_core::ActivatedInvocation {
            invocation: inv,
            activation,
            classified,
        };
        let text = json(&delivered).map_err(|_| AppRefusal::Unsupported)?;
        let answered = self
            .ask(app, Wake::Start, budget(within), |p| async move {
                p.perform(&text, &Details::new()).await
            })
            .await;
        match answered {
            Ok(text) => read::<Result<Outcome, AppRefusal>>(&text)
                .map_err(|_| AppFault::Refused(gone()))?
                .map_err(AppFault::Refused),
            Err(LinkFault::Timeout) => Err(AppFault::TimedOut),
            Err(LinkFault::Unavailable) => Err(AppFault::Unavailable),
            Err(LinkFault::Malformed) => Err(AppFault::Refused(gone())),
        }
    }

    async fn dry_run(&self, app: &AppName, inv: Invocation) -> Result<Preview, AppRefusal> {
        let text = json(&inv).map_err(|_| AppRefusal::Unsupported)?;
        let answered = self
            .ask(app, Wake::Start, QUICK, |p| async move {
                p.dry_run(&text, &Details::new()).await
            })
            .await;
        match answered {
            Ok(text) => read::<Result<Preview, AppRefusal>>(&text).map_err(|_| gone())?,
            Err(_) => Err(gone()),
        }
    }

    async fn undo(&self, app: &AppName, token: &UndoToken, actor: &Actor) -> Result<(), UndoFault> {
        let (Ok(token), Ok(actor)) = (json(token), json(actor)) else {
            return Err(UndoFault::AppUnavailable);
        };
        let answered = self
            .ask(app, Wake::Start, QUICK, |p| async move {
                p.undo(&token, &actor).await
            })
            .await;
        match answered {
            Ok(text) => {
                read::<Result<(), UndoFault>>(&text).map_err(|_| UndoFault::AppUnavailable)?
            }
            Err(_) => Err(UndoFault::AppUnavailable),
        }
    }

    async fn running(&self, app: &AppName) -> bool {
        let Ok(name) = BusName::try_from(app.to_string()) else {
            return false;
        };
        match DBusProxy::new(&self.connection).await {
            Ok(dbus) => dbus.name_has_owner(name).await.unwrap_or(false),
            Err(_) => false,
        }
    }

    async fn context(
        &self,
        app: &AppName,
        scope: ContextScope,
    ) -> Result<ContextSnapshot, LinkFault> {
        let scope = json(&scope)?;
        let text = self
            .ask(app, Wake::IfRunning, READ, |p| async move {
                p.context(&scope).await
            })
            .await?;
        read(&text)
    }

    async fn preview(&self, app: &AppName, id: &EntityId) -> Result<Preview, LinkFault> {
        let id = json(id)?;
        let text = self
            .ask(app, Wake::IfRunning, READ, |p| async move {
                p.preview(&id).await
            })
            .await?;
        read(&text)
    }

    async fn suggest(&self, app: &AppName, ask: SuggestAsk) -> Result<Vec<EntityRef>, LinkFault> {
        let ask = json(&ask)?;
        let text = self
            .ask(
                app,
                Wake::Start,
                READ,
                |p| async move { p.suggest(&ask).await },
            )
            .await?;
        read(&text)
    }
}
