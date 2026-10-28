//! A helper has one conversation. When the companion opens it a second time (after a restart it
//! has forgotten the first) the router hands back the one it already has, live or stored, so no
//! helper ever has two.

use crate::router::Router;
use crate::seams::Seams;
use crate::session::SessionState;
use companion_model::helper::{HelperSeat, SeatPlace};
use docket_session::{PageSize, Read, SessionEntry, SessionLog, Standing};
use docket_wire::{CallerRole, IntentsReply, SessionOpen, SessionOpened};
use porter_core::Count;
use prov::{HelperId, SessionId};

/// Rows read from the head of a log to find its seat: the opening, then the seat.
const HEAD: PageSize = PageSize(Count(2));

/// The helper whose conversation `open` asks for, when it asks for one.
fn conversation_of(open: &SessionOpen) -> Option<&HelperId> {
    open.helper
        .as_ref()
        .filter(|seat| seat.place == SeatPlace::Conversation)
        .map(|seat| &seat.helper)
}

impl<S: Seams> Router<S> {
    /// The conversation `open` would make, when the helper already has one: live and not closed,
    /// or stored and brought back now. `None` when the open is for anything else, or the helper has
    /// none. Only the companion's open counts, as for the seat itself.
    pub(crate) async fn existing_conversation(
        &self,
        role: CallerRole,
        open: &SessionOpen,
    ) -> Option<IntentsReply> {
        let helper = conversation_of(open).filter(|_| role == CallerRole::Companion)?;
        if let Some(opened) = self.live_conversation(helper) {
            return Some(IntentsReply::SessionOpened(opened));
        }
        let stored = self.stored_conversation(helper).await?;
        Some(IntentsReply::SessionOpened(stored))
    }

    fn live_conversation(&self, helper: &HelperId) -> Option<SessionOpened> {
        self.locked().sessions.iter().find_map(|(id, record)| {
            let seat = record.helper.as_ref()?;
            let kept = seat.helper == *helper
                && seat.place == SeatPlace::Conversation
                && !matches!(record.state, SessionState::Closed(_));
            kept.then(|| SessionOpened {
                session: id.clone(),
                task: record.task.clone(),
                mode: record.mode,
            })
        })
    }

    async fn stored_conversation(&self, helper: &HelperId) -> Option<SessionOpened> {
        let all = self.seams.log().sessions().await.ok()?;
        for id in all {
            let held = self.locked().sessions.contains_key(&id);
            if held || !self.seated_for(&id, helper).await {
                continue;
            }
            let Ok(restored) = self.restore_session(&id).await else {
                continue;
            };
            if matches!(restored.standing, Standing::Open | Standing::Paused(_)) {
                let (task, mode) = {
                    let st = self.locked();
                    let record = st.sessions.get(&id)?;
                    (record.task.clone(), record.mode)
                };
                return Some(SessionOpened {
                    session: id,
                    task,
                    mode,
                });
            }
        }
        None
    }

    /// Whether the head of `id`'s log gives `helper` a conversation seat.
    async fn seated_for(&self, id: &SessionId, helper: &HelperId) -> bool {
        let Ok(page) = self.seams.log().page(id, None, HEAD).await else {
            return false;
        };
        page.rows.iter().any(|row| match &row.read {
            Read::Entry(entry) => matches!(
                entry.as_ref(),
                SessionEntry::Seat(HelperSeat { helper: h, place: SeatPlace::Conversation, .. })
                    if h == helper
            ),
            _ => false,
        })
    }
}
