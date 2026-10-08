//! The router's sheet, put to the editor's person. The host hands the sheet over as an event; the
//! editor's click goes back to the host as a `SheetChoice`, and the host (which holds the router's
//! seam, clock and receipts) turns it into the router's answer. This file only carries: the
//! router decides what "always" covers and whether it was offered.

use crate::out;
use crate::permission::{self, choice_of_reply};
use crate::prompt::Reply;
use crate::server::{Server, Ticks};
use crate::turn::{Order, Turn};
use crate::wire::{Wire, WireClosed};
use docket_core::ConfirmRequest;
use docket_session::{SessionHost, SessionLog};
use prov::SessionId;

impl<H: SessionHost, L: SessionLog, W: Wire, T: Ticks> Server<H, L, W, T> {
    /// Asks the editor about `sheet` and gives the host its choice. A cancel while it waits
    /// answers nothing: the host cancels the turn and the router withdraws the sheet.
    pub(crate) async fn put_sheet(
        &mut self,
        session: &SessionId,
        sheet: &ConfirmRequest,
        turn: &mut Turn,
    ) -> Result<Vec<Order>, WireClosed> {
        let request = permission::sheet_request(&out::wire_id(session), sheet);
        match self.ask_editor(session, &request, turn).await? {
            Reply::Heard(orders) => Ok(orders),
            Reply::Answer(reply) => {
                let choice = choice_of_reply(reply, sheet);
                // A host that cannot take it leaves the sheet to expire on its own terms.
                let _ = self.host.answer_sheet(session, &sheet.id, choice).await;
                turn.sheet_answered(choice);
                Ok(Vec::new())
            }
        }
    }
}
