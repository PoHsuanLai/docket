//! How on-screen runs end, heard from the run daemon. `Cua1.Run.Finished` says by an enumerated
//! kind why a run did not finish; the companion turns it into fixed words on the run's roster
//! line. Only the owner of the daemon's bus name is heard, and only its enumerated fields are
//! read: what the run wrote in its report is not in the signal and never reaches the planner
//! except as a handle.

use crate::Companiond;
use docket_client::Transport as IntentsTransport;
use docket_dbus::BusConnection;
use docket_tasks::report_end;
use futures_util::StreamExt;
use futures_util::lock::Mutex;
use porter_client::Transport as InferTransport;
use std::sync::Arc;
use zbus::fdo::DBusProxy;
use zbus::message::Type;
use zbus::names::BusName;
use zbus::{MatchRule, MessageStream};

/// The run daemon's bus name.
const CUA_BUS: &str = "org.quire.Cua1";

fn rule() -> Option<MatchRule<'static>> {
    Some(
        MatchRule::builder()
            .msg_type(Type::Signal)
            .interface("org.quire.Cua1.Run")
            .ok()?
            .member("Finished")
            .ok()?
            .build(),
    )
}

/// Listens for runs finishing until the connection closes.
pub(crate) async fn watch<P, I>(connection: BusConnection, companion: Arc<Mutex<Companiond<P, I>>>)
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let (Some(rule), Ok(dbus), Ok(name)) = (
        rule(),
        DBusProxy::new(&connection).await,
        BusName::try_from(CUA_BUS),
    ) else {
        return;
    };
    let Ok(mut signals) = MessageStream::for_match_rule(rule, &connection, Some(64)).await else {
        return;
    };
    while let Some(Ok(message)) = signals.next().await {
        let sender = message.header().sender().map(|s| s.to_string());
        let owner = dbus
            .get_name_owner(name.clone())
            .await
            .ok()
            .map(|o| o.to_string());
        if sender.is_none() || sender != owner {
            continue;
        }
        let Ok(text) = message.body().deserialize::<String>() else {
            continue;
        };
        if let Some((report, why)) = report_end(&text) {
            companion.lock().await.run_ended(report, why);
        }
    }
}
