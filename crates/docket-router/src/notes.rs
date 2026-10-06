//! What the companion hands the router to keep or to do about its own sessions: the episodes and
//! narratives of its tasks and the records it keeps of them (`Session.Note`), the narrowing of a
//! subagent's policy from the person's words (`Session.Narrow`), and what a session holds by
//! handle (`Session.Handles`).

use crate::router::Router;
use crate::seams::{Clock, EventSink, Seams};
use crate::tasks::{TaskRecord, TaskState};
use almanac_core::{Episode, EpisodeId, EpisodeKind, EpisodeOutcome, Narrative};
use docket_core::{AuditRecord, IntentsReply, NoteAsk, WireRefusal, close};
use prov::{Integrity, ReportStatus, SessionId};

fn refuse(why: WireRefusal) -> IntentsReply {
    IntentsReply::Refused(why)
}

/// How a task that ended is an episode's outcome.
fn outcome_of(status: ReportStatus) -> EpisodeOutcome {
    match status {
        ReportStatus::Done | ReportStatus::Progress => EpisodeOutcome::Done,
        ReportStatus::Failed => EpisodeOutcome::Failed,
        ReportStatus::Cancelled => EpisodeOutcome::Cancelled,
    }
}

/// The episode the router keeps of an ended task: its own skeleton, from its own ledger.
pub(crate) fn skeleton_episode(
    task: &TaskRecord,
    at: prov::UnixSeconds,
    status: ReportStatus,
) -> Option<Episode> {
    close(&task.ledger, EpisodeKind::Task, at, outcome_of(status))
}

impl<S: Seams> Router<S> {
    /// `.Session.Note`.
    pub(crate) fn session_note(&self, id: &SessionId, note: NoteAsk) -> IntentsReply {
        let now = self.seams.clock().now();
        let Some(space) = self.locked().sessions.get(id).map(|r| r.space.clone()) else {
            return refuse(WireRefusal::NoSuchSession);
        };
        match note {
            // An episode in another Space, or one whose skeleton is not the router's own kind of
            // words, is not a thing this session may leave.
            NoteAsk::Episode(episode) => {
                if episode.space != space || episode.skeleton.label.integrity != Integrity::Trusted
                {
                    return refuse(WireRefusal::NotAllowed);
                }
                self.seams.sink().append(AuditRecord::Episode(episode));
            }
            NoteAsk::Narrative { episode, narrative } => {
                match self.narrated(&space, &episode, narrative, now) {
                    Some(narrated) => self
                        .seams
                        .sink()
                        .append(AuditRecord::Episode(Box::new(narrated))),
                    None => return refuse(WireRefusal::Malformed),
                }
            }
            NoteAsk::End => {
                let ended = {
                    let mut st = self.locked();
                    let task = st.sessions.get(id).map(|r| r.task.clone());
                    task.and_then(|task| {
                        st.tasks
                            .get_mut(&task)
                            .and_then(|t| crate::opening::end_task(t, now))
                    })
                };
                if let Some(episode) = ended {
                    self.seams
                        .sink()
                        .append(AuditRecord::Episode(Box::new(episode)));
                }
            }
            NoteAsk::Record(note) => self.seams.sink().append(AuditRecord::Session {
                at: now,
                space,
                slug: note.slug,
                json: note.json,
            }),
        }
        IntentsReply::Done
    }

    /// The router's own skeleton of an ended task of `space`, with `narrative` added: the
    /// narrated successor of the episode it already recorded.
    fn narrated(
        &self,
        space: &prov::SpaceId,
        episode: &EpisodeId,
        narrative: Narrative,
        now: prov::UnixSeconds,
    ) -> Option<Episode> {
        let st = self.locked();
        let task = st
            .tasks
            .get(&prov::TaskId::parse(episode.as_str()).ok()?)
            .filter(|t| &t.space == space)?;
        let TaskState::Ended(status) = task.state else {
            return None;
        };
        let mut episode = skeleton_episode(task, now, status)?;
        episode.narrative = Some(narrative);
        Some(episode)
    }

    /// `.Session.Handles`: what the session holds by handle, in shape only.
    pub(crate) fn session_handles(&self, id: &SessionId) -> IntentsReply {
        match self.locked().sessions.get(id) {
            Some(record) => IntentsReply::Handles(record.handles.cards()),
            None => refuse(WireRefusal::NoSuchSession),
        }
    }
}
