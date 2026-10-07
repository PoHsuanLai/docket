//! A pressed card's call, followed to its end on the bus: the answer shows each step the router
//! reports. The companion is locked only to write, never while waiting on the router or the
//! person.

use crate::{Bell, Clock};
use docket_client::{PerformEvent, Transport as IntentsTransport};
use docket_tasks::{Acting, Companion, refusal_of};
use porter_client::Transport as InferTransport;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Reads the card's request to its end, telling the answer at each step.
pub(crate) async fn follow<P, I>(
    companion: Arc<Mutex<Companion<P, I, Clock, Bell>>>,
    mut acting: Acting,
) where
    P: InferTransport,
    I: IntentsTransport,
{
    let result = loop {
        match acting.event().await {
            Ok(PerformEvent::Progress(progress)) => {
                companion.lock().await.act_progress(&acting, &progress);
            }
            Ok(PerformEvent::Done(end)) => break *end,
            Err(error) => break Err(refusal_of(error)),
        }
    };
    companion.lock().await.act_end(&acting, result);
}
