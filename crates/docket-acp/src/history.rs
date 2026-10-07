//! A stored session as the editor sees it: a line for `session/list` and the replay `session/load`
//! sends before it answers. Pure: rows and plans in, protocol values out. Only the person's own
//! words and our step lines are replayed; handles stay handles and nothing is re-read.

use crate::calls;
use agent_client_protocol_schema::v1::{SessionInfo, SessionUpdate};
use docket_session::{Logged, Read, ResumePlan, SessionEntry};
use prov::{SessionId, UnixSeconds};

/// How many characters of the first prompt make a title.
const TITLE: usize = 80;

/// `at` as an RFC 3339 UTC time.
pub fn rfc3339(at: UnixSeconds) -> String {
    let secs = at.0.rem_euclid(86_400);
    let days = at.0.div_euclid(86_400) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        secs % 3_600 / 60,
        secs % 60
    )
}

/// The `session/list` line of a session an editor opened, or none when it has no workspace.
pub fn info(session: &SessionId, plan: &ResumePlan) -> Option<SessionInfo> {
    let cwd = plan.opening.cwd.as_ref()?;
    let title: Option<String> = plan
        .turns
        .first()
        .map(|t| t.text.chars().take(TITLE).collect());
    let updated = plan.turns.last().map(|t| rfc3339(t.at));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn times_are_rfc3339_utc() {
        let cases = [
            (0, "1970-01-01T00:00:00Z"),
            (951_782_400, "2000-02-29T00:00:00Z"),
            (1_760_000_000, "2025-10-09T08:53:20Z"),
            (-1, "1969-12-31T23:59:59Z"),
        ];
        for (at, want) in cases {
            assert_eq!(rfc3339(UnixSeconds(at)), want);
        }
    }
}
