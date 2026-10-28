//! A stored session as the editor sees it: a line for `session/list` and the replay `session/load`
//! sends before it answers. Pure: rows and plans in, protocol values out. Only the person's own
//! words and our step lines are replayed; handles stay handles and nothing is re-read.

use crate::calls;
use agent_client_protocol::schema::v1::{SessionInfo, SessionUpdate};
use docket_session::{Logged, Read, ResumePlan, SessionEntry};
use prov::{SessionId, UnixSeconds};

/// How many characters of the first prompt make a title.
const TITLE: usize = 80;

/// The `session/list` line of a session an editor opened, or none when it has no workspace.
pub fn info(session: &SessionId, plan: &ResumePlan) -> Option<SessionInfo> {
    let cwd = plan.opening.cwd.as_ref()?;
    let title: Option<String> = plan
        .turns
        .first()
        .map(|t| t.text.chars().take(TITLE).collect());
    let updated = plan
        .turns
        .last()
        .and_then(|t| docket_vocab::when::utc_instant(t.at));
    Some(
        SessionInfo::new(crate::out::wire_id(session), cwd.as_str())
            .title(title)
            .updated_at(updated),
    )
}

/// When the session was last spoken in: the key `session/list` sorts by, newest first.
pub fn last_spoken(plan: &ResumePlan) -> UnixSeconds {
    plan.turns.last().map_or(UnixSeconds(0), |t| t.at)
}

/// The updates that replay the session: its turns and calls in log order, then the calls a
/// restart cut off, ended as failed.
pub fn replay(rows: &[Logged], plan: &ResumePlan) -> Vec<SessionUpdate> {
    let mut updates = Vec::new();
    for row in rows {
        let Read::Entry(entry) = &row.read else {
            continue;
        };
        match entry.as_ref() {
            SessionEntry::Turn(turn) => updates.push(calls::said(turn.text.clone())),
            SessionEntry::Call(open) => updates.push(calls::started(open)),
            SessionEntry::Step(step) => updates.push(calls::ended(step)),
            _ => {}
        }
    }
    for cut in &plan.interrupted {
        updates.push(calls::stopped(
            &cut.open,
            "Interrupted by a restart; it may have run.",
        ));
    }
    updates
}
