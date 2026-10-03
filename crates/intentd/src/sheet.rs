//! The sheet that asks the person: `org.quire.Confirm1`, served by sill. Only the process that
//! owns the name and plays the `confirm` role in `intentd.toml` is trusted with a question: a
//! name nobody trusted answers nothing, and an unanswered sheet is a dismissal, never an allow.

use crate::config::IntentdConfig;
use crate::peer::Peers;
use docket_core::{CallerRole, ConfirmAnswer, ConfirmEnd, ConfirmId, ConfirmRequest, Confirmer};
use docket_dbus::{BusConnection, CONFIRM_BUS, ConfirmProxy};
use std::sync::Arc;
use std::time::Duration;

/// How long past the sheet's own expiry intentd waits before it stops: sill that never answers
/// must not hold a call open for ever.
const GRACE: Duration = Duration::from_secs(10);

/// Asks through sill's `Confirm1` (a sheet in phase A, the trusted surface in phase B). The
/// receipt's input proof says which; a spoken answer is never one.
#[derive(Debug, Clone)]
pub struct SheetConfirmer {
    connection: BusConnection,
    peers: Peers,
    grace: Duration,
}

impl SheetConfirmer {
    /// Asks over `connection`, trusting the `confirm` role of the shipped configuration.
    pub fn new(connection: BusConnection) -> Self {
        let config = IntentdConfig::shipped().unwrap_or_else(|_| IntentdConfig::empty());
        Self::trusting(connection, Arc::new(config))
    }

    /// Asks over `connection`, trusting the `confirm` role of `config`.
    pub fn trusting(connection: BusConnection, config: Arc<IntentdConfig>) -> Self {
        let peers = Peers::new(connection.clone(), config);
        Self {
            connection,
            peers,
            grace: GRACE,
        }
    }

    /// Waits `grace` past a sheet's own expiry before ending it as expired.
    pub fn waiting(mut self, grace: Duration) -> Self {
        self.grace = grace;
        self
    }

    /// Whether the owner of `Confirm1` is the one process intentd trusts to draw the sheet.
    async fn trusted(&self) -> bool {
        self.peers
            .owner_of(CONFIRM_BUS)
            .await
            .is_ok_and(|owner| owner.roles.contains(&CallerRole::Confirm))
    }
}

fn dismissed() -> ConfirmAnswer {
    ConfirmAnswer::Ended(ConfirmEnd::Dismissed)
}

impl Confirmer for SheetConfirmer {
    async fn confirm(&self, request: ConfirmRequest) -> ConfirmAnswer {
        if !self.trusted().await {
            return dismissed();
        }
        let Ok(text) = serde_json::to_string(&request) else {
            return dismissed();
        };
        let Ok(proxy) = ConfirmProxy::new(&self.connection).await else {
            return dismissed();
        };
        let wait = Duration::from_secs(u64::from(request.expires.0)) + self.grace;
        let asked = docket_client::requested(&self.connection, CONFIRM_BUS, proxy.confirm(&text));
        match tokio::time::timeout(wait, asked).await {
            Err(_) => ConfirmAnswer::Ended(ConfirmEnd::Expired),
            Ok(Ok(Ok((0, reply)))) => serde_json::from_str(&reply).unwrap_or_else(|_| dismissed()),
            Ok(Ok(Ok((1, _)))) => ConfirmAnswer::Ended(ConfirmEnd::Cancelled),
            Ok(_) => dismissed(),
        }
    }

    async fn cancel(&self, id: &ConfirmId) {
        if !self.trusted().await {
            return;
        }
        if let Ok(proxy) = ConfirmProxy::new(&self.connection).await {
            let _ = proxy.cancel(id.as_str()).await;
        }
    }
}
