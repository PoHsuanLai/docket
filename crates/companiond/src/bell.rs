//! The surface of the daemon: changes fan out to the bus's signal tasks over a broadcast channel,
//! an interactive request wakes the idle pass through a `Notify`, and an answer's address is its
//! object path. This is the one place the task model meets tokio.

use docket_tasks::{Change, Surface};
use prov::TaskId;
use std::future::Future;
use tokio::sync::{Notify, broadcast};

/// What the bus's tasks listen on.
#[derive(Debug)]
pub struct Bell {
    changes: broadcast::Sender<Change>,
    interrupt: Notify,
}

impl Default for Bell {
    fn default() -> Self {
        let (changes, _) = broadcast::channel(256);
        Self {
            changes,
            interrupt: Notify::new(),
        }
    }
}

impl Surface for Bell {
    type Changes = broadcast::Receiver<Change>;

    fn changed(&self, change: Change) {
        // No subscriber is not an error: nobody is listening yet.
        let _ = self.changes.send(change);
    }

    fn changes(&self) -> Self::Changes {
        self.changes.subscribe()
    }

    fn interrupt(&self) {
        self.interrupt.notify_waiters();
    }

    fn interrupted(&self) -> impl Future<Output = ()> + Send {
        self.interrupt.notified()
    }

    fn answer_path(&self, task: &TaskId) -> String {
        docket_dbus::answer_path(task)
    }
}
