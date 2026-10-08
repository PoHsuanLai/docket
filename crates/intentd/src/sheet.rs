//! The sheet that asks the person: `org.quire.Confirm1`, served by sill. Only the process that
//! owns the name and plays the `confirm` role in `intentd.toml` is trusted with a question: a
//! name nobody trusted answers nothing, and an unanswered sheet is a dismissal, never an allow.
//!
//! A sheet for a call in a session an editor drives (`ConfirmRequest::editor`, set by the router)
//! goes to that editor's process instead, which serves the same `Confirm1` under its own bus name
//! and is trusted only while it plays the `editor` role. If it is gone, the sheet ends as expired:
//! it is not moved to the desktop, where the person would be asked about a call whose editor has
//! left, with nothing waiting to receive the answer.
//!
//! The same route serves the host of an external coding agent that asked, when it opened the
//! session, to show that session's sheets itself (`docket-agent --tty`, a development fallback):
//! its process is trusted there while it plays the `acp_agent` role. Without that ask the router
//! names no route, and the agent's sheets are the desktop's like any other.

use crate::acp_gate::AcpGate;
use crate::config::IntentdConfig;
use crate::peer::Peers;
use crate::procroot::ProcRoot;
use docket_core::{CallerRole, ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmRequest, Confirmer};
use docket_dbus::{BusConnection, CONFIRM_BUS, ConfirmProxy};
use porter_core::AppName;
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

/// How long past the sheet's own expiry intentd waits before it stops: sill that never answers
/// must not hold a call open for ever.
const GRACE: Duration = Duration::from_secs(10);

/// Where a sheet is drawn.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Surface {
    /// sill's `Confirm1`.
    Desktop,
    /// The process of the editor the person is speaking through, which serves `Confirm1` under its
    /// own bus name.
    Editor(AppName),
}

impl Surface {
    /// The surface for `request`: its editor, when the router named one that is an app name.
    fn of(request: &ConfirmRequest) -> Self {
        request
            .editor
            .as_ref()
            .and_then(|route| AppName::parse(route.client.as_str()).ok())
            .map_or(Surface::Desktop, Surface::Editor)
    }

    fn service(&self) -> &str {
        match self {
            Surface::Desktop => CONFIRM_BUS,
            Surface::Editor(app) => app.as_str(),
        }
    }

    /// The roles of which the owner of the service must play one to be trusted with a question.
    fn roles(&self) -> &'static [CallerRole] {
        match self {
            Surface::Desktop => &[CallerRole::Confirm],
            Surface::Editor(_) => &[CallerRole::Editor, CallerRole::AcpAgent],
        }
    }

    /// How a sheet that got no answer from this surface ended: the desktop's person dismissed it;
    /// an editor that is gone (or not trusted) let it expire. Never an allow, and never moved to
    /// the desktop (see FINDINGS, "editor sheets over the bus").
    fn unanswered(&self) -> ConfirmAnswer {
        ConfirmAnswer::Ended(match self {
            Surface::Desktop => ConfirmEnd::Dismissed,
            Surface::Editor(_) => ConfirmEnd::Expired,
        })
    }
}

/// Asks through sill's `Confirm1` (a sheet in phase A, the trusted surface in phase B), or, for a
/// call in a session an editor drives, through the `Confirm1` the editor's process serves. The
/// receipt's input proof says which; a spoken answer is never one.
#[derive(Debug, Clone)]
pub struct SheetConfirmer {
    connection: BusConnection,
    config: Arc<IntentdConfig>,
    peers: Peers,
    grace: Duration,
    /// The surface each open sheet went to, so a withdrawal finds it.
    open: Arc<Mutex<BTreeMap<ConfirmId, Surface>>>,
}

impl SheetConfirmer {
    /// Asks over `connection`, trusting the `confirm` role of the shipped configuration.
    pub fn new(connection: BusConnection) -> Self {
        let config = IntentdConfig::shipped().unwrap_or_else(|_| IntentdConfig::empty());
        Self::trusting(connection, Arc::new(config))
    }

    /// Asks over `connection`, trusting the `confirm` role of `config`.
    pub fn trusting(connection: BusConnection, config: Arc<IntentdConfig>) -> Self {
        let peers = Peers::new(connection.clone(), config.clone());
        Self {
            connection,
            config,
            peers,
            grace: GRACE,
            open: Arc::default(),
        }
    }

    /// Trusts the editor role of the name `org.quire.Acp` only while `acp` shows the setting on.
    pub fn gated(mut self, acp: AcpGate, proc_root: &ProcRoot) -> Self {
        self.peers = Peers::gated(self.connection.clone(), self.config.clone(), proc_root, acp);
        self
    }

    /// Waits `grace` past a sheet's own expiry before ending it as expired.
    pub fn waiting(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    fn remember(&self, id: &ConfirmId, surface: Option<Surface>) -> Option<Surface> {
        let mut open = self.open.lock().unwrap_or_else(PoisonError::into_inner);
        match surface {
            Some(surface) => open.insert(id.clone(), surface),
            None => open.remove(id),
        }
    }

    /// Whether the owner of `surface`'s service is the one process intentd trusts to draw there.
    async fn trusted(&self, surface: &Surface) -> bool {
        self.peers
            .owner_of(surface.service())
            .await
            .is_ok_and(|owner| surface.roles().iter().any(|r| owner.roles.contains(r)))
    }

    async fn proxy(&self, surface: &Surface) -> Option<ConfirmProxy<'static>> {
        ConfirmProxy::builder(&self.connection)
            .destination(surface.service().to_owned())
            .ok()?
            .build()
            .await
            .ok()
    }

    async fn ask(&self, surface: &Surface, request: &ConfirmRequest) -> ConfirmAnswer {
        let Ok(text) = serde_json::to_string(request) else {
            return surface.unanswered();
        };
        let Some(proxy) = self.proxy(surface).await else {
            return surface.unanswered();
        };
        let wait = Duration::from_secs(u64::from(request.expires.0)) + self.grace;
        let asked =
            docket_client::requested(&self.connection, surface.service(), proxy.confirm(&text));
        match tokio::time::timeout(wait, asked).await {
            Err(_) => ConfirmAnswer::Ended(ConfirmEnd::Expired),
            Ok(Ok(Ok((0, reply)))) => {
                serde_json::from_str(&reply).unwrap_or_else(|_| surface.unanswered())
            }
            Ok(Ok(Ok((1, _)))) => ConfirmAnswer::Ended(ConfirmEnd::Cancelled),
            Ok(_) => surface.unanswered(),
        }
    }
}

impl Confirmer for SheetConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        let surface = Surface::of(&request);
        if !self.trusted(&surface).await {
            return surface.unanswered();
        }
        self.remember(&request.id, Some(surface.clone()));
        let answer = self.ask(&surface, &request).await;
        self.remember(&request.id, None);
        answer
    }

    async fn cancel(&self, id: &ConfirmId) {
        let surface = self.remember(id, None).unwrap_or(Surface::Desktop);
        if !self.trusted(&surface).await {
            return;
        }
        if let Some(proxy) = self.proxy(&surface).await {
            let _ = proxy.cancel(id.as_str()).await;
        }
    }
}
