//! The records companiond keeps of its own sessions (`companion-wire`'s `SessionRecord`), written
//! through the router (`Session.Note`) so a restart can rebuild the roster and the front task.
//! A record that cannot be written costs only that rebuild: nothing waits on it.

use crate::runtime::Companiond;
use almanac_core::JsonText;
use companion_wire::SessionRecord;
use docket_client::Transport as IntentsTransport;
use docket_core::{NoteAsk, NoteSlug, SessionNote, TaskLedger, skeleton_of};
use porter_client::Transport as InferTransport;
use prov::{SessionId, TaskId};

/// The record as the router stores it: its slug and its serde JSON.
pub(crate) fn note_of(record: &SessionRecord) -> Option<NoteAsk> {
    let slug = NoteSlug::parse(record.slug()).ok()?;
    let json = JsonText::parse(&serde_json::to_string(record).ok()?).ok()?;
    Some(NoteAsk::Record(SessionNote { slug, json }))
}

impl<P: InferTransport, I: IntentsTransport> Companiond<P, I> {
    /// Writes `record` for `session`. A router that does not answer is not waited for twice.
    pub(crate) async fn record(&self, session: &SessionId, record: &SessionRecord) {
        if let Some(note) = note_of(record) {
            let _ = self.intents.session_note(session.clone(), note).await;
        }
    }

    /// What the task has done so far, as a typed digest: the skeleton of its ledger.
    pub(crate) fn digest_of(&self, task: &TaskId) -> Option<almanac_core::Skeleton> {
        let rt = self.runtimes.get(task)?;
        Some(skeleton_of(&TaskLedger {
            task: task.clone(),
            agent: rt.agent.clone(),
            parent: rt
                .parent
                .as_ref()
                .and_then(|p| almanac_core::EpisodeId::parse(p.as_str()).ok()),
            space: rt.space.clone(),
            started: rt.started,
            asked: rt.turns.clone(),
            steps: rt.steps.clone(),
            touched: vec![],
            results: vec![],
        }))
    }
}
