//! The helpers seam (design note `swarm.md`, section 7.2): what an editor may see and do of the
//! person's helpers. The server holds no helper state of its own. It asks this seam which helpers
//! there are, what their recent jobs were, and hears from it what the board says; the helper's
//! conversation and job sessions live in the same `SessionHost` as every other session, so a turn,
//! a call and a sheet go the way they always do.

use companion_wire::{JobLine, ToldWire};
use docket_core::StandingScope;
use docket_core::helper::HelperName;
use prov::{HelperId, SessionId, SpaceId};
use std::future::Future;

/// A helper as the editor sees it: one session, titled with its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRow {
    /// The helper.
    pub id: HelperId,
    /// What the person calls it: the session's title.
    pub name: HelperName,
    /// Its Space. The editor sees it only when the Space is the one its workspace maps to.
    pub space: SpaceId,
    /// The session the person's words to the helper (a side conversation) are recorded on.
    pub session: SessionId,
}

/// A request the helpers could not take, in a sentence the person reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HelperRefusal(pub String);

/// The person's helpers, as far as an editor may reach them. Nothing here lets the editor answer a
/// sheet of ours or widen a helper: the only authority it can add is a grant for that helper.
pub trait Helpers: Send {
    /// Every helper there is. The server keeps those in the editor's workspace's Space.
    fn rows(&self) -> Vec<HelperRow>;

    /// The jobs of `helper`, oldest first.
    fn jobs(&self, helper: &HelperId) -> Vec<JobLine>;

    /// Opens a job session for `helper` and names it. The host the server drives resumes it.
    fn open_job(
        &mut self,
        helper: &HelperId,
    ) -> impl Future<Output = Result<SessionId, HelperRefusal>> + Send;

    /// The next sentence the board tells about one of the `watching` helpers (those the editor
    /// has a session for). It pends while there is none. The server drops it when a line from
    /// the editor comes first, so it must lose nothing when dropped.
    fn next_told(&mut self, watching: &[HelperId]) -> impl Future<Output = ToldWire> + Send;

    /// The editor's person said "always" on a sheet of `helper`'s: a standing grant for exactly
    /// `scope`, held for that helper alone.
    fn always(
        &mut self,
        helper: &HelperId,
        scope: StandingScope,
    ) -> impl Future<Output = ()> + Send;
}

/// No helpers: the server as it was before they existed.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHelpers;

impl Helpers for NoHelpers {
    fn rows(&self) -> Vec<HelperRow> {
        Vec::new()
    }

    fn jobs(&self, _helper: &HelperId) -> Vec<JobLine> {
        Vec::new()
    }

    async fn open_job(&mut self, _helper: &HelperId) -> Result<SessionId, HelperRefusal> {
        Err(HelperRefusal("There are no helpers here.".to_owned()))
    }

    async fn next_told(&mut self, _watching: &[HelperId]) -> ToldWire {
        std::future::pending().await
    }

    async fn always(&mut self, _helper: &HelperId, _scope: StandingScope) {}
}
