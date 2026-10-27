//! A pressed card's call, followed to its end on the bus: the answer shows each step the router
//! reports. The companion is locked only to write, never while waiting on the router or the
//! person.

use crate::Bell;
use docket_client::{PerformEvent, Transport as IntentsTransport};
use docket_core::Clock;
use docket_planner::Planner;
use docket_tasks::{Acting, Companion, refusal_of};
use futures_util::lock::Mutex;
use std::sync::Arc;

/// Reads the card's request to its end, telling the answer at each step.
pub(crate) async fn follow<P, I, K>(
    companion: Arc<Mutex<Companion<P, I, K, Bell>>>,
    mut acting: Acting,
) where
    P: Planner,
    I: IntentsTransport,
    K: Clock,
{
    let result = loop {
        match tokio::time::timeout(acting.patience(), acting.event()).await {
            Ok(Ok(PerformEvent::Progress(progress))) => {
                companion.lock().await.act_progress(&acting, &progress);
            }
            Ok(Ok(PerformEvent::Done(end))) => break *end,
            Ok(Err(error)) => break Err(refusal_of(error)),
            // The router said nothing for the whole span: the card's call ends unanswered.
            Err(_) => break Err(docket_core::CallRefusal::Timeout),
        }
    };
    companion.lock().await.act_end(&acting, result);
}
