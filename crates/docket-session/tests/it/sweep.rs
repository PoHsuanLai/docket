//! The crash-point sweep: a crash can end the log after any append. Every prefix of a log
//! written in write-ahead order resumes, never loses taint, and never re-runs a call.

use crate::support::*;
use docket_session::*;
use proptest::prelude::*;
use std::collections::BTreeSet;

#[test]
fn every_prefix_of_a_canonical_log_resumes_safely() {
    let log = canonical();
    let mut before = Taint::Clean;
    for cut in 1..=log.len() {
        let kept = &log[..cut];
        let p = resume_plan(&rows(kept)).expect("a prefix with its opening has a plan");
        assert!(
            p.faults.is_empty(),
            "cut {cut}: write-ahead order has no fault"
        );
        assert!(p.taint >= before, "cut {cut}: taint went down");
        before = p.taint;

        let began: BTreeSet<u64> = kept
            .iter()
            .filter_map(|e| match e {
                SessionEntry::Call(c) => Some(c.call.0),
                _ => None,
            })
            .collect();
        let ended: BTreeSet<u64> = kept
            .iter()
            .filter_map(|e| match e {
                SessionEntry::Step(s) => Some(s.call.0),
                _ => None,
            })
            .collect();
        let cut_off: BTreeSet<u64> = p.interrupted.iter().map(|i| i.call.0).collect();
        assert_eq!(cut_off, &began - &ended, "cut {cut}: in-flight calls");
        assert!(p.history.iter().all(|s| !cut_off.contains(&s.call.0)));

        let last_policy = kept.iter().rev().find_map(|e| match e {
            SessionEntry::Policy(p) => Some(p.clone()),
            _ => None,
        });
        assert_eq!(p.policy, last_policy, "cut {cut}: policy as stored");
        let closed = kept.iter().any(|e| matches!(e, SessionEntry::Closed(_)));
        assert_eq!(matches!(p.standing, Standing::Closed(_)), closed);
    }
}

#[test]
fn a_crash_before_the_write_ahead_taint_cannot_clear_it() {
    // The handle's entry reached the log and the taint did not (the order was broken):
    // the plan is Tainted and says why.
    let broken = [SessionEntry::Opened(opening()), untrusted_handle(1)];
    let p = resume_plan(&rows(&broken)).expect("plan");
    assert_eq!(p.taint, Taint::Tainted);
    assert_eq!(p.faults, vec![ResumeFault::MissingTaint { at: Seq(1) }]);
}

#[derive(Debug, Clone, Copy)]
enum Op {
    Turn,
    Policy,
    Begin,
    End,
    Taint,
    Untrusted,
    Trusted,
    Trip,
    Reset,
    Budget,
    Close,
}

fn entry_of(op: Op, n: u64) -> SessionEntry {
    match op {
        Op::Turn => SessionEntry::Turn(turn(n, "words")),
        Op::Policy => SessionEntry::Policy(policy()),
        Op::Begin => call(n, "mail.thread.search"),
        Op::End => step(n, "mail.thread.search"),
        Op::Taint => taint(),
        Op::Untrusted => untrusted_handle(n),
        Op::Trusted => trusted_handle(n),
        Op::Trip => SessionEntry::Breaker(BreakerNote::Tripped(docket_core::BreakerTrip::Recent)),
        Op::Reset => SessionEntry::Breaker(BreakerNote::Reset),
        Op::Budget => SessionEntry::Budget(ledger(n as u32)),
        Op::Close => SessionEntry::Closed(EndCause::Closed),
    }
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        Just(Op::Turn),
        Just(Op::Policy),
        Just(Op::Begin),
        Just(Op::End),
        Just(Op::Taint),
        Just(Op::Untrusted),
        Just(Op::Trusted),
        Just(Op::Trip),
        Just(Op::Reset),
        Just(Op::Budget),
        Just(Op::Close),
    ]
}

proptest! {
    #[test]
    fn taint_is_monotone_over_any_log(ops in prop::collection::vec(op(), 0..40)) {
        let mut log = vec![SessionEntry::Opened(opening())];
        log.extend(ops.iter().enumerate().map(|(i, o)| entry_of(*o, i as u64 + 1)));
        let mut before = Taint::Clean;
        let mut seen_taint = false;
        for cut in 1..=log.len() {
            let p = resume_plan(&rows(&log[..cut])).expect("plan");
            prop_assert!(p.taint >= before);
            before = p.taint;
            seen_taint |= matches!(log[cut - 1], SessionEntry::Taint(_));
            // Any taint entry, or any untrusted handle, makes the plan Tainted.
            let untrusted = log[..cut].iter().any(|e| matches!(e,
                SessionEntry::Handle(h) if h.label.integrity == prov::Integrity::Untrusted));
            if seen_taint || untrusted {
                prop_assert_eq!(p.taint, Taint::Tainted);
            }
        }
    }
}
