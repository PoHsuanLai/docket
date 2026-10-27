//! The object companiond calls: `org.quire.AgentHost1`, open to that one caller.

use super::Service;
use docket_core::HandOver;
use docket_core::helper::AppsAsk;
use docket_dbus::callers::{COMPANIOND, Identity, require};
use std::sync::Arc;
use zbus::fdo;
use zbus::message::Header;

/// The object companiond calls.
pub(super) struct Object {
    pub(super) service: Arc<Service>,
    pub(super) callers: Arc<Identity>,
}

#[zbus::interface(name = "org.quire.AgentHost1")]
impl Object {
    async fn hand_over(
        &self,
        handover: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        let ask: HandOver =
            serde_json::from_str(&handover).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        Ok(self.service.begin(ask))
    }

    async fn wait(&self, turn: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        match self.service.wait(&turn).await {
            Ok(Some(reply)) => {
                serde_json::to_string(&reply).map_err(|e| fdo::Error::Failed(e.to_string()))
            }
            Ok(None) => Ok(String::new()),
            Err(no_such) => Err(fdo::Error::InvalidArgs(no_such.to_string())),
        }
    }

    async fn apps_begin(
        &self,
        ask: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        let ask: AppsAsk =
            serde_json::from_str(&ask).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.service.remember_caller(sender);
        Ok(self.service.apps_begin(ask))
    }

    async fn apps_wait(
        &self,
        call: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        match self.service.apps_wait(&call).await {
            Ok(Some(reply)) => {
                serde_json::to_string(&reply).map_err(|e| fdo::Error::Failed(e.to_string()))
            }
            Ok(None) => Ok(String::new()),
            Err(no_such) => Err(fdo::Error::InvalidArgs(no_such.to_string())),
        }
    }
}
