//! `resume_plan`: the fold of a session's log into a `ResumePlan`. Pure and total over any
//! rows. The rules, from the design note's section 5:
//!
//! 1. Taint is the join of every `Taint` entry and of every untrusted handle; it never goes
//!    down. An untrusted handle with no taint written before it is a fault, and the plan is
//!    Tainted (fail closed).
//! 2. A `Call` with no `Step` is interrupted and never re-run.
//! 3. The policy is the last `Policy` entry as stored; nothing derives it again.
//! 4. Handles come back as labels.
//! 5. `Compacted` is a note on top of the entries it covers, which stay in the plan; the latest
//!    whole one is the summary. It never lowers taint, and a summary written while the session was
//!    tainted is untrusted. A summary that covers more than the log held, or less than an earlier
//!    one, is a fault and is ignored.
//! 6. `Checkpoint` and entries of a kind this build does not know are ignored.
//! 7. `Closed` stays closed; a breaker trip holds until a `Turn`; a gap or an entry that cannot
//!    be read blocks new turns and taints.

use crate::codec::{Logged, Read};
use crate::entry::{
    BreakerNote, CallOpen, Compacted, EndCause, HandleLabel, Opening, Seq, SessionEntry, Taint,
};
use crate::plan::{
    Blocker, Interrupted, PlanRefusal, ResumeFault, ResumePlan, ResumedBudget, Standing,
};
use docket_core::helper::HelperSeat;
use docket_core::{
    BreakerTrip, CallId, Handle, Ledger, SkillId, SkillVersion, StepLine, Summary, TaskPolicy,
    UserTurn,
};
use porter_core::Count;
use prov::Integrity;
use std::collections::BTreeMap;

/// The fold's running state.
struct Fold {
    opening: Opening,
    seat: Option<HelperSeat>,
    taint: Taint,
    policy: Option<TaskPolicy>,
    turns: Vec<UserTurn>,
    history: Vec<StepLine>,
    summary: Option<Summary>,
    open_calls: BTreeMap<CallId, CallOpen>,
    handles: BTreeMap<Handle, HandleLabel>,
    ledger: Option<Ledger>,
    calls_since: u32,
    tripped: Option<BreakerTrip>,
    closed: Option<EndCause>,
    skills: Vec<(SkillId, SkillVersion)>,
    legacy_skipped: u32,
    blocker: Option<Blocker>,
    faults: Vec<ResumeFault>,
}

impl Fold {
    fn new(opening: Opening) -> Self {
        Fold {
            opening,
            seat: None,
            taint: Taint::Clean,
            policy: None,
            turns: Vec::new(),
            history: Vec::new(),
            summary: None,
            open_calls: BTreeMap::new(),
            handles: BTreeMap::new(),
            ledger: None,
            calls_since: 0,
            tripped: None,
            closed: None,
            skills: Vec::new(),
            legacy_skipped: 0,
            blocker: None,
            faults: Vec::new(),
        }
    }

    /// Something that may hide a taint: the session is Tainted from here on.
    fn block(&mut self, why: Blocker, fault: ResumeFault) {
        self.taint = Taint::Tainted;
        self.blocker.get_or_insert(why);
        self.faults.push(fault);
    }

    fn apply(&mut self, at: Seq, entry: SessionEntry) {
        match entry {
            SessionEntry::Opened(_) => self.faults.push(ResumeFault::DuplicateOpened { at }),
            SessionEntry::Seat(seat) => {
                self.seat.get_or_insert(seat);
            }
            SessionEntry::Turn(turn) => {
                self.tripped = None;
                self.turns.push(turn);
            }
            SessionEntry::Policy(policy) => self.policy = Some(policy),
            SessionEntry::Call(open) => {
                self.calls_since = self.calls_since.saturating_add(1);
                self.open_calls.insert(open.call, open);
            }
            SessionEntry::Step(line) => {
                self.open_calls.remove(&line.call);
                self.history.push(line);
            }
            SessionEntry::Handle(label) => {
                let untrusted = label.label.integrity == Integrity::Untrusted;
                if untrusted && self.taint == Taint::Clean {
                    self.faults.push(ResumeFault::MissingTaint { at });
                    self.taint = Taint::Tainted;
                }
                self.handles.insert(label.handle, label);
            }
            SessionEntry::Taint(_) => self.taint = Taint::Tainted,
            SessionEntry::Breaker(BreakerNote::Tripped(trip)) => self.tripped = Some(trip),
            SessionEntry::Breaker(BreakerNote::Reset) => self.tripped = None,
            SessionEntry::Budget(ledger) => {
                self.ledger = Some(ledger);
                self.calls_since = 0;
            }
            SessionEntry::Skill(skill) => {
                if !self.skills.iter().any(|(id, _)| *id == skill.id) {
                    self.skills.push((skill.id, skill.version));
                }
            }
            SessionEntry::Closed(cause) => {
                self.closed.get_or_insert(cause);
            }
            SessionEntry::Compacted(note) => self.compact(at, note),
            // Restore points do not touch the tail of a resumed session; a kind a newer build
            // wrote is kept in the log and read as nothing here.
            SessionEntry::Checkpoint(_) | SessionEntry::Unknown(_) => {}
        }
    }

    /// A summary: kept when it covers no more than the log held before it and no less than the
    /// last one. Its taint joins the session's, and a summary never reads as more trusted than
    /// the session was.
    fn compact(&mut self, at: Seq, note: Compacted) {
        if note.taint == Taint::Tainted && self.taint == Taint::Clean {
            self.faults.push(ResumeFault::MissingTaint { at });
        }
        self.taint = self.taint.join(note.taint);
        let held = (self.turns.len(), self.history.len());
        let covers = note.covers;
        let before = self.summary.as_ref().map_or((0, 0), |s| {
            (s.covers.turns.0 as usize, s.covers.steps.0 as usize)
        });
        let wants = (covers.turns.0 as usize, covers.steps.0 as usize);
        if wants.0 > held.0 || wants.1 > held.1 || wants.0 < before.0 || wants.1 < before.1 {
            self.faults.push(ResumeFault::BadCompaction { at });
            return;
        }
        let integrity = match self.taint {
            Taint::Clean => Integrity::Trusted,
            Taint::Tainted => Integrity::Untrusted,
        };
        self.summary = Some(Summary {
            covers,
            text: note.text,
            integrity,
        });
    }

    fn standing(&self) -> Standing {
        match (&self.closed, &self.blocker, self.tripped) {
            (Some(cause), _, _) => Standing::Closed(*cause),
            (None, Some(why), _) => Standing::Blocked(why.clone()),
            (None, None, Some(trip)) => Standing::Paused(trip),
            (None, None, None) => Standing::Open,
        }
    }

    fn plan(self) -> ResumePlan {
        let standing = self.standing();
        ResumePlan {
            opening: self.opening,
            seat: self.seat,
            standing,
            taint: self.taint,
            policy: self.policy,
            turns: self.turns,
            history: self.history,
            summary: self.summary,
            interrupted: self
                .open_calls
                .into_iter()
                .map(|(call, open)| Interrupted { call, open })
                .collect(),
            handles: self.handles.into_values().collect(),
            budget: ResumedBudget {
                checkpoint: self.ledger,
                calls_since: Count(self.calls_since),
            },
            skills: self.skills,
            legacy_skipped: Count(self.legacy_skipped),
            faults: self.faults,
        }
    }
}

/// The plan a session resumes from, or why there is none.
pub fn resume_plan(rows: &[Logged]) -> Result<ResumePlan, PlanRefusal> {
    let first = rows.first().ok_or(PlanRefusal::Empty)?;
    let Read::Entry(entry) = &first.read else {
        return Err(PlanRefusal::NoOpening);
    };
    let SessionEntry::Opened(opening) = entry.as_ref() else {
        return Err(PlanRefusal::NoOpening);
    };
    let mut fold = Fold::new(opening.clone());
    let mut expected = first.seq.next();
    for row in &rows[1..] {
        if row.seq != expected {
            fold.block(
                Blocker::Gap,
                ResumeFault::Gap {
                    expected,
                    found: row.seq,
                },
            );
        }
        expected = row.seq.next();
        match &row.read {
            Read::Entry(entry) => fold.apply(row.seq, entry.as_ref().clone()),
            Read::Legacy(_) => fold.legacy_skipped = fold.legacy_skipped.saturating_add(1),
            Read::Unreadable(why) => fold.block(
                Blocker::Unreadable,
                ResumeFault::Unreadable {
                    at: row.seq,
                    why: why.clone(),
                },
            ),
        }
    }
    Ok(fold.plan())
}
