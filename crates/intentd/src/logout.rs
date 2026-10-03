//! The end of the person's session: the terminal's standing grants last "until logout", so the
//! daemon calls `Router::end_terminal_sessions` when logind says the session is removed. With
//! no session id known any session's removal counts: ending a grant early only makes the next
//! call from the terminal ask again. The unit is also `PartOf=graphical-session.target`, so the
//! daemon (and everything it holds in memory) stops with the graphical session.

use crate::serve::ServeFault;
use docket_dbus::BusConnection;
use docket_router::{Router, Seams};
use futures_util::StreamExt;
use std::sync::Arc;
use zbus::zvariant::ObjectPath;

#[zbus::proxy(
    interface = "org.freedesktop.login1.Manager",
    default_service = "org.freedesktop.login1",
    default_path = "/org/freedesktop/login1"
)]
trait Login1Manager {
    #[zbus(signal)]
    fn session_removed(&self, id: &str, path: ObjectPath<'_>) -> zbus::Result<()>;
}

/// A subscription to logind's `SessionRemoved`. It exists once the match rule is installed on
/// the bus, so a removal after `watch_logind` returns is never missed.
#[derive(Debug)]
pub struct Logouts {
    removed: SessionRemovedStream,
}

/// Subscribes to `SessionRemoved` of logind on `system`.
pub async fn watch_logind(system: &BusConnection) -> Result<Logouts, ServeFault> {
    let bus = |e: zbus::Error| ServeFault::Bus(e.to_string());
    let manager = Login1ManagerProxy::new(system).await.map_err(bus)?;
    let removed = manager.receive_session_removed().await.map_err(bus)?;
    Ok(Logouts { removed })
}

impl Logouts {
    /// Ends the terminal sessions of `router` whenever `session` is removed, or any session when
    /// it is none. Runs until the connection closes.
    pub async fn end_terminals<S: Seams + 'static>(
        mut self,
        router: Arc<Router<S>>,
        session: Option<String>,
    ) {
        while let Some(signal) = self.removed.next().await {
            let Ok(args) = signal.args() else { continue };
            if session.as_deref().is_none_or(|ours| ours == args.id) {
                router.end_terminal_sessions();
            }
        }
    }
}

/// `watch_logind`, then `end_terminals`, in one call.
pub async fn end_terminals_at_logout<S: Seams + 'static>(
    system: &BusConnection,
    router: Arc<Router<S>>,
    session: Option<String>,
) -> Result<(), ServeFault> {
    watch_logind(system)
        .await?
        .end_terminals(router, session)
        .await;
    Ok(())
}
