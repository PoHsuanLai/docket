//! What the served companion does beside the bus calls: listens for a message that arrived for
//! an agent, and tells the companion the time on a beat.

use crate::Companiond;
use docket_client::Transport as IntentsTransport;
use docket_core::Clock;
use docket_dbus::{BusConnection, INTENTS_BUS, MessageProxy};
use docket_planner::Planner;
use futures_util::lock::Mutex;
use std::sync::Arc;
use std::time::Duration;

/// How often time is told to the companion: side conversations end and the idle pass starts on
/// this beat.
const TICK: Duration = Duration::from_secs(5);

/// A message arrived for an agent: read the inbox of the agents the companion runs.
pub(crate) async fn arrivals<P, I, K>(
    connection: BusConnection,
    companion: Arc<Mutex<Companiond<P, I, K>>>,
) where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
{
    use futures_util::StreamExt;
    let Ok(proxy) = MessageProxy::builder(&connection)
        .destination(INTENTS_BUS)
        .and_then(|b| b.path(docket_dbus::INTENTS_PATH))
    else {
        return;
    };
    let Ok(proxy) = proxy.build().await else {
        return;
    };
    let Ok(mut signals) = proxy.receive_arrived().await else {
        return;
    };
    while signals.next().await.is_some() {
        let _ = companion.lock().await.arrived().await;
    }
}

pub(crate) async fn ticking<P, I, K>(companion: Arc<Mutex<Companiond<P, I, K>>>)
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
{
    let mut beat = tokio::time::interval(TICK);
    loop {
        beat.tick().await;
        // A turn in flight holds the companion: time is told again on the next beat, never
        // queued behind it.
        if let Some(mut held) = companion.try_lock() {
            let _ = held.tick().await;
        }
    }
}
