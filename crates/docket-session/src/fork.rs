//! `fork`: a new session's first entries, cut from a parent's log at a position. The child
//! inherits taint and the breaker (a fork must not launder either), keeps the person's words,
//! the policy as stored (never wider) and the handle labels, and starts with fresh budgets
//! (no `Budget` entry; the Space's cap is the Space's, not the session's). It is open: a
//! `Closed` is not copied, so reopening a closed session is a fork.

use crate::codec::{Logged, Read};
use crate::entry::{ForkPoint, Seq, SessionEntry, Taint, TaintCause, TaintNote};
use crate::plan::{PlanRefusal, Standing};
use crate::resume::resume_plan;
use prov::{SessionId, TaskId};

/// Why a log cannot be forked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ForkFault {
    /// The parent has no entry at that position.
    #[error("the parent has no entry at that position")]
    PastEnd,
    /// The kept entries are not a session.
    #[error("the kept entries have no plan: {0}")]
    Plan(#[from] PlanRefusal),
    /// The kept entries have a gap or one that cannot be read: a fork would copy a guess.
    #[error("the parent's log is not whole up to that position")]
    NotWhole,
}

/// The child's log from position 0: `Opened` (naming `task` and the fork point), a `Taint` when
/// the parent's plan is tainted (first, so write-ahead holds for every handle after it), then
/// the parent's entries up to and including `at`, minus `Opened`, `Taint`, `Budget` and
/// `Closed`.
pub fn fork(
    parent: &SessionId,
    rows: &[Logged],
    at: Seq,
    task: TaskId,
) -> Result<Vec<SessionEntry>, ForkFault> {
    let kept: Vec<Logged> = rows.iter().filter(|r| r.seq <= at).cloned().collect();
    if kept.last().map(|r| r.seq) != Some(at) {
        return Err(ForkFault::PastEnd);
    }
    let plan = resume_plan(&kept)?;
    if matches!(plan.standing, Standing::Blocked(_)) {
        return Err(ForkFault::NotWhole);
    }
    let mut opening = plan.opening;
    opening.task = task;
    opening.forked_from = Some(ForkPoint {
        session: parent.clone(),
        at,
    });
    let mut child = vec![SessionEntry::Opened(opening)];
    if plan.taint == Taint::Tainted {
        child.push(SessionEntry::Taint(TaintNote {
            cause: TaintCause::Inherited,
            at_call: None,
        }));
    }
    let carried = kept.into_iter().skip(1).filter_map(|r| match r.read {
        Read::Entry(e) => Some(*e),
        Read::Legacy(_) | Read::Unreadable(_) => None,
    });
    child.extend(carried.filter(|e| {
        !matches!(
            e,
            SessionEntry::Opened(_)
                | SessionEntry::Taint(_)
                | SessionEntry::Budget(_)
                | SessionEntry::Closed(_)
        )
    }));
    Ok(child)
}
