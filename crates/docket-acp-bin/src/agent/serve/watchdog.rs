//! When the host ends: it has been quiet for [`IDLE`] with no turn running, or the daemon that
//! asked for the helpers' agents has left the bus.

use super::Service;
use std::time::{Duration, Instant};
use zbus::fdo;

/// How long the host keeps an agent's session open with nothing asked of it.
const IDLE: Duration = Duration::from_secs(10 * 60);

/// How often the host looks at the clock for that.
const WATCH: Duration = Duration::from_secs(20);

impl Service {
    /// Notes that the host was just asked for something.
    pub(super) fn touch(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = Instant::now();
        }
    }

    fn quiet_for(&self) -> Duration {
        self.last
            .lock()
            .map_or(Duration::ZERO, |last| last.elapsed())
    }

    /// Returns when the host should end: nothing has been asked of it for [`IDLE`] and no turn
    /// is running, or the daemon that asked for the helpers' agents has left.
    pub async fn until_done(&self) {
        loop {
            tokio::time::sleep(WATCH).await;
            if self.quiet_for() > IDLE && self.assistant.try_lock().is_ok() && !self.helpers_busy()
            {
                return;
            }
            if self.caller_gone().await {
                return;
            }
        }
    }

    /// Waits a moment for the replies nobody has collected yet, so a caller still asking is not
    /// left with a host that is gone: at most ten seconds.
    pub async fn drain(&self) {
        for _ in 0..20 {
            if self.turns.is_empty() && self.apps.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Whether a call on the helpers' agents is running.
    fn helpers_busy(&self) -> bool {
        self.helpers.get().is_some_and(|helpers| helpers.busy())
    }

    /// Notes who asked for the helpers' agents, to end them when that caller is gone.
    pub fn remember_caller(&self, sender: Option<&str>) {
        if let (Some(sender), Ok(mut caller)) = (sender, self.caller.lock()) {
            *caller = Some(sender.to_owned());
        }
    }

    /// Whether the daemon that asked for the helpers' agents has left the bus: no agent process
    /// outlives it.
    async fn caller_gone(&self) -> bool {
        let Some(name) = self.caller.lock().ok().and_then(|caller| caller.clone()) else {
            return false;
        };
        let (Ok(bus), Ok(name)) = (
            fdo::DBusProxy::new(&self.bus).await,
            zbus::names::BusName::try_from(name),
        ) else {
            return false;
        };
        matches!(bus.name_has_owner(name).await, Ok(false))
    }
}
