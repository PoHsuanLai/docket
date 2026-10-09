//! A scripted `org.quire.Confirm1` server: sill's role in the acceptance run. It owns the two
//! names sill owns (`org.quire.Confirm1` and `org.quire.Shell`), so intentd derives the
//! `confirm` role for it from `intentd.toml` exactly as it would for sill. Every request it is
//! shown is kept for the test and handed out on a channel; the verdict comes from a queue the
//! test fills (the sheet's answer: Allow with a receipt, or the person saying no).

use docket_core::{ConfirmAnswer, ConfirmEnd, ConfirmRequest};
use docket_dbus::{CONFIRM_BUS, CONFIRM_PATH};
use porter_core::consent::GrantScope;
use prov::{Confidentiality, ConfirmReceipt, Effect, InputProof, UnixSeconds};
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
    /// Presses Allow and "always": the grant is recorded and the same ask is not made again.
    AllowAlways,
    /// Presses Refuse.
    Refuse,
}

/// What the person does with every sheet once the queue is empty, by what the sheet is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByEffect {
    /// For a sheet about a read-only action.
    pub reads: Verdict,
    /// For every other sheet.
    pub rest: Verdict,
}

impl ByEffect {
    fn verdict_for(self, request: &ConfirmRequest) -> Verdict {
        if request.effect == Effect::Read {
            self.reads
        } else {
            self.rest
        }
    }
}

/// When the person's answer reaches intentd.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Release {
    /// As soon as the sheet is shown.
    AtOnce,
    /// Only when the test calls [`Sheet::release`]: the sheet is seen waiting first.
    WhenReleased,
}

/// An answer the test holds back until [`Sheet::release`].
#[derive(Debug)]
struct Held {
    connection: zbus::Connection,
    caller: zbus::names::UniqueName<'static>,
    path: String,
    reply: String,
}

#[derive(Debug, Default)]
struct Inner {
    by_effect: Option<ByEffect>,
    verdicts: VecDeque<(Verdict, Release)>,
    held: Vec<Held>,
    shown: Vec<ConfirmRequest>,
    cancelled: Vec<String>,
    next: u32,
}

/// The server's handle: the test queues verdicts and reads what was shown.
#[derive(Debug, Clone)]
pub struct Sheet {
    inner: Arc<Mutex<Inner>>,
    held: Arc<tokio::sync::Notify>,
}

impl Sheet {
    fn edit<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> R {
        f(&mut self.inner.lock().unwrap_or_else(PoisonError::into_inner))
    }

    /// Queues what the person does with the next sheet that has no verdict yet.
    pub fn will(&self, verdict: Verdict) {
        self.edit(|i| i.verdicts.push_back((verdict, Release::AtOnce)));
    }

    /// Queues what the person does with the next sheet, but holds the answer until
    /// [`Sheet::release`], so the test can see the turn waiting on the sheet first.
    pub fn will_when_released(&self, verdict: Verdict) {
        self.edit(|i| i.verdicts.push_back((verdict, Release::WhenReleased)));
    }

    /// Sends every answer held back by [`Sheet::will_when_released`], first waiting for one to be
    /// held: the turn shows that it waits on the sheet a moment before the sheet is asked.
    pub async fn release(&self) {
        let all = loop {
            let all = self.edit(|i| std::mem::take(&mut i.held));
            if !all.is_empty() {
                break all;
            }
            self.held.notified().await;
        };
        for held in all {
            let _ = held
                .connection
                .emit_signal(
                    Some(held.caller),
                    held.path,
                    "org.quire.Intents1.Request",
                    "Response",
                    &(0_u32, held.reply),
                )
                .await;
        }
    }

    /// Sets what the person does with each sheet the queue does not answer, by effect.
    pub fn will_by_effect(&self, rule: ByEffect) {
        self.edit(|i| i.by_effect = Some(rule));
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
        Verdict::Allow | Verdict::AllowAlways => ConfirmAnswer::Allowed {
            scope: match verdict {
                Verdict::AllowAlways => GrantScope::Always,
                _ => GrantScope::Once,
            },
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
            (
                i.next,
                i.verdicts.pop_front().or_else(|| {
                    i.by_effect
                        .map(|r| (r.verdict_for(&parsed), Release::AtOnce))
                }),
            )
        });
        let _ = self.seen.send(parsed.clone());
        let path = format!("{CONFIRM_PATH}/request/{n}");
        let object = ObjectPath::try_from(path.as_str())
            .map_err(|e| fdo::Error::Failed(e.to_string()))?
            .into();
        let caller = header.sender().map(|s| s.to_owned());
        // No verdict queued: the sheet stays open until intentd withdraws it.
        if let (Some((verdict, release)), Some(caller)) = (verdict, caller) {
            let reply = serde_json::to_string(&answer_of(verdict, &parsed))
                .map_err(|e| fdo::Error::Failed(e.to_string()))?;
            let connection = connection.clone();
            if release == Release::WhenReleased {
                self.sheet.edit(|i| {
                    i.held.push(Held {
                        connection,
                        caller,
                        path,
                        reply,
                    })
                });
                self.sheet.held.notify_one();
                return Ok(object);
            }
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
        held: Arc::default(),
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
