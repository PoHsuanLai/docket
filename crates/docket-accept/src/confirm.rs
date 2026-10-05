//! A scripted `org.quire.Confirm1` server: sill's role in the acceptance run. It owns the two
//! names sill owns (`org.quire.Confirm1` and `org.quire.Shell`), so intentd derives the
//! `confirm` role for it from `intentd.toml` exactly as it would for sill. Every request it is
//! shown is kept for the test and handed out on a channel; the verdict comes from a queue the
//! test fills (the sheet's answer: Allow with a receipt, or the person saying no).

use docket_core::{ConfirmAnswer, ConfirmEnd, ConfirmRequest};
use docket_dbus::{CONFIRM_BUS, CONFIRM_PATH};
use porter_core::consent::GrantScope;
use prov::{Confidentiality, ConfirmReceipt, InputProof, UnixSeconds};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, PoisonError};
use tokio::sync::mpsc;
use zbus::fdo;
use zbus::message::Header;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

/// What the person does with the next sheet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Presses Allow, once.
    Allow,
    /// Presses Refuse.
    Refuse,
}

#[derive(Debug, Default)]
struct Inner {
    verdicts: VecDeque<Verdict>,
    shown: Vec<ConfirmRequest>,
    cancelled: Vec<String>,
    next: u32,
}

/// The server's handle: the test queues verdicts and reads what was shown.
#[derive(Debug, Clone)]
pub struct Sheet {
    inner: Arc<Mutex<Inner>>,
}

impl Sheet {
    fn edit<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Queues what the person does with the next sheet that has no verdict yet.
    pub fn will(&self, verdict: Verdict) {
        self.edit(|i| i.verdicts.push_back(verdict));
    }

    /// Every request shown so far, oldest first.
    pub fn shown(&self) -> Vec<ConfirmRequest> {
        self.edit(|i| i.shown.clone())
    }

    /// The ids intentd withdrew.
    pub fn cancelled(&self) -> Vec<String> {
        self.edit(|i| i.cancelled.clone())
    }
}

struct ConfirmObject {
    sheet: Sheet,
    seen: mpsc::UnboundedSender<ConfirmRequest>,
}

fn answer_of(verdict: Verdict, request: &ConfirmRequest) -> ConfirmAnswer {
    match verdict {
        Verdict::Allow => ConfirmAnswer::Allowed {
            scope: GrantScope::Once,
            receipt: ConfirmReceipt {
                id: request.id.clone(),
                input: InputProof::SheetFallback,
                at: UnixSeconds(1),
                covers: Confidentiality::Secret,
            },
        },
        Verdict::Refuse => ConfirmAnswer::Ended(ConfirmEnd::Refused),
    }
}

#[zbus::interface(name = "org.quire.Confirm1")]
impl ConfirmObject {
    async fn confirm(
        &self,
        request: String,
        #[zbus(header)] header: Header<'_>,
        #[zbus(connection)] connection: &zbus::Connection,
    ) -> fdo::Result<OwnedObjectPath> {
        let parsed: ConfirmRequest =
            serde_json::from_str(&request).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let (n, verdict) = self.sheet.edit(|i| {
            i.shown.push(parsed.clone());
            i.next += 1;
            (i.next, i.verdicts.pop_front())
        });
        let _ = self.seen.send(parsed.clone());
        let path = format!("{CONFIRM_PATH}/request/{n}");
        let object = ObjectPath::try_from(path.as_str())
            .map_err(|e| fdo::Error::Failed(e.to_string()))?
            .into();
        let caller = header.sender().map(|s| s.to_owned());
        // No verdict queued: the sheet stays open until intentd withdraws it.
        if let (Some(verdict), Some(caller)) = (verdict, caller) {
            let reply = serde_json::to_string(&answer_of(verdict, &parsed))
                .map_err(|e| fdo::Error::Failed(e.to_string()))?;
            let connection = connection.clone();
            tokio::spawn(async move {
                let _ = connection
                    .emit_signal(
                        Some(caller),
                        path,
                        "org.quire.Intents1.Request",
                        "Response",
                        &(0_u32, reply),
                    )
                    .await;
            });
        }
        Ok(object)
    }

    async fn cancel(&self, id: String) {
        self.sheet.edit(|i| i.cancelled.push(id));
    }
}

/// Serves `Confirm1` on `connection` and claims sill's two names there. Returns the handle and
/// the channel on which each request shown arrives.
pub async fn serve(
    connection: &zbus::Connection,
) -> zbus::Result<(Sheet, mpsc::UnboundedReceiver<ConfirmRequest>)> {
    let sheet = Sheet {
        inner: Arc::default(),
    };
    let (seen, requests) = mpsc::unbounded_channel();
    connection
        .object_server()
        .at(
            CONFIRM_PATH,
            ConfirmObject {
                sheet: sheet.clone(),
                seen,
            },
        )
        .await?;
    connection.request_name(CONFIRM_BUS).await?;
    connection.request_name("org.quire.Shell").await?;
    Ok((sheet, requests))
}
