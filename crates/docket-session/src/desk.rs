//! Where an editor session's sheets wait. The router asks its confirmer; for a session an editor
//! drives, a host that has a desk hands the request out as an event (`BackendEvent::Sheet`) and
//! answers it from the person's choice. A host with no desk (`NoDesk`) sends nothing: the sheet
//! stays wherever the router's confirmer puts it, and the editor is told it is waiting there.

use crate::backend::SheetChoice;
use docket_core::{ConfirmId, ConfirmRequest};
use prov::SessionId;
use std::future::Future;

/// Why a sheet could not be answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DeskFault {
    /// The desk holds no open sheet by that id (answered, withdrawn, or never its own).
    #[error("no such sheet")]
    NoSuchSheet,
}

/// The sheets the router has put to the host and nobody has answered.
pub trait SheetDesk: Send + Sync {
    /// The open sheet `id`, as the router asked it.
    fn request(&self, id: &ConfirmId) -> Option<ConfirmRequest>;

    /// The next sheet of `session` the host has not been handed, waiting for one. A sheet is
    /// handed out once. `None` at once from a desk that never holds any (it is not an editor's).
    /// Cancel-safe: a future dropped before it is ready hands out nothing.
    fn next_sheet(
        &self,
        session: &SessionId,
    ) -> impl Future<Output = Option<ConfirmRequest>> + Send {
        let _ = session;
        std::future::ready(None)
    }

    /// Answers `id` with the person's choice. The router's own offer decides what an "always"
    /// covers; the choice names no scope.
    fn answer(&self, id: &ConfirmId, choice: SheetChoice) -> Result<(), DeskFault>;
}

/// A desk with nothing on it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct NoDesk;

impl SheetDesk for NoDesk {
    fn request(&self, _id: &ConfirmId) -> Option<ConfirmRequest> {
        None
    }

    fn answer(&self, _id: &ConfirmId, _choice: SheetChoice) -> Result<(), DeskFault> {
        Err(DeskFault::NoSuchSheet)
    }
}
