//! `org.quire.Confirm1` served by the docket-acp process, under its own bus name. intentd hands a
//! sheet for a call in an editor's session here instead of to sill (`ConfirmRequest::editor`,
//! trusted only while this name plays the `editor` role); the sheet waits on the [`EditorDesk`],
//! the server shows it to the editor as an ACP permission request, and the editor's choice comes
//! back through the host as the answer, with a receipt minted here (`SheetConfirmer`).
//!
//! Only intentd may ask or withdraw: a call from any other process is refused, so another
//! process cannot put sheets on the editor or take them down. Intentd is the process
//! `acp-callers.toml` names as the router (the `agent` role of the app `org.quire.Intents`, by its
//! unit's main process), never whoever owns a well-known name.

use docket_core::{ConfirmId, ConfirmRequest, Confirmer};
use docket_dbus::CONFIRM_PATH;
use docket_dbus::callers::{INTENTD, Identity, require};
use docket_host::{EditorDesk, SheetConfirmer};
use std::sync::Arc;
use zbus::fdo;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

/// The sheets intentd put to this process, answered through the shared desk.
#[derive(Debug)]
pub struct ConfirmObject<K: docket_core::Clock + 'static> {
    sheets: Arc<SheetConfirmer<EditorDesk, K>>,
    next: std::sync::atomic::AtomicU64,
    callers: Arc<Identity>,
}

impl<K: docket_core::Clock + 'static> ConfirmObject<K> {
    /// Answers for the sheets on `desk`, stamping receipts with `clock`, to the process that
    /// `callers` names as intentd.
    pub fn new(desk: EditorDesk, clock: K, callers: Arc<Identity>) -> Self {
        Self {
            sheets: Arc::new(SheetConfirmer::new(desk, clock)),
            next: std::sync::atomic::AtomicU64::new(0),
            callers,
        }
    }
}

/// Whether `header` is a message from the process the callers table names as intentd: the only
/// one that may ask or withdraw a sheet.
async fn from_intentd(header: &Header<'_>, callers: &Identity) -> fdo::Result<()> {
    let sender = header.sender().map(|s| s.as_str());
    require(callers, sender, &[INTENTD]).await?;
    Ok(())
}

#[zbus::interface(name = "org.quire.Confirm1")]
impl<K: docket_core::Clock + 'static> ConfirmObject<K> {
    async fn confirm(
        &self,
        request: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<OwnedObjectPath> {
        from_intentd(&header, &self.callers).await?;
        let parsed: ConfirmRequest =
            serde_json::from_str(&request).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let n = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let path = format!("{CONFIRM_PATH}/request/{n}");
        let object: OwnedObjectPath = ObjectPath::try_from(path.as_str())
            .map_err(|e| fdo::Error::Failed(e.to_string()))?
            .into();
        let caller = header.sender().map(|s| s.to_owned());
        let sheets = self.sheets.clone();
        let connection = connection.clone();
        // The sheet goes on the desk when the task first runs; the host waits for it if the
        // router's own signal got there first. The answer follows as the Request's `Response`.
        tokio::spawn(async move {
            let answer = sheets.confirm(parsed).await;
            let Some(caller) = caller else { return };
            if let Ok(reply) = serde_json::to_string(&answer) {
                // The router stopped listening when it withdrew the sheet; nothing to tell.
                let _ = connection
                    .emit_signal(
                        Some(caller),
                        path,
                        "org.quire.Intents1.Request",
                        "Response",
                        &(0_u32, reply),
                    )
                    .await;
            }
        });
        Ok(object)
    }

    async fn cancel(&self, id: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        from_intentd(&header, &self.callers).await?;
        let id = ConfirmId::parse(&id).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.sheets.cancel(&id).await;
        Ok(())
    }
}
