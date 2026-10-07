use crate::support::*;
use docket_core::BreakerTrip;
use docket_session::*;

fn plan(entries: &[SessionEntry]) -> ResumePlan {
    resume_plan(&rows(entries)).expect("a plan")
}

#[test]
fn a_whole_session_restores_what_was_stored() {
    let p = plan(&canonical());
    assert_eq!(p.standing, Standing::Closed(EndCause::Closed));
    assert_eq!(p.taint, Taint::Tainted);
    assert_eq!(p.policy, Some(policy()));
    assert_eq!(p.turns.len(), 2);
    assert_eq!(p.history.len(), 2);
    assert!(p.interrupted.is_empty() && p.faults.is_empty());
    assert_eq!(p.handles.len(), 2);
    assert_eq!(p.budget.checkpoint, Some(ledger(2)));
    assert_eq!(p.budget.calls_since.0, 0);
}

#[test]
fn refusals_name_what_is_missing() {
    assert_eq!(resume_plan(&[]), Err(PlanRefusal::Empty));
    let no_opening = rows(&[SessionEntry::Turn(turn(1, "hi"))]);
    assert_eq!(resume_plan(&no_opening), Err(PlanRefusal::NoOpening));
}

#[test]
fn a_call_without_its_step_is_interrupted_and_not_in_history() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        call(1, "mail.thread.archive"),
        call(2, "mail.thread.search"),
        step(2, "mail.thread.search"),
    ]);
    assert_eq!(p.interrupted.len(), 1);
    assert_eq!(p.interrupted[0].call.0, 1);
    assert_eq!(p.history.len(), 1);
    assert_eq!(p.standing, Standing::Open);
    assert_eq!(p.budget.calls_since.0, 2);
}

#[test]
fn an_interrupted_call_stays_listed_after_a_close() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        call(1, "mail.thread.archive"),
        SessionEntry::Closed(EndCause::SpaceHalted),
    ]);
    assert_eq!(p.interrupted.len(), 1);
    assert_eq!(p.standing, Standing::Closed(EndCause::SpaceHalted));
}

#[test]
fn the_policy_is_the_last_one_stored_and_never_derived() {
    let mut narrower = policy();
    narrower.max_count = porter_core::Count(1);
    let p = plan(&[
        SessionEntry::Opened(opening()),
        SessionEntry::Policy(policy()),
        SessionEntry::Policy(narrower.clone()),
    ]);
    assert_eq!(p.policy, Some(narrower));
    assert_eq!(plan(&[SessionEntry::Opened(opening())]).policy, None);
}

#[test]
fn a_trip_holds_until_a_turn_or_a_reset() {
    let tripped = SessionEntry::Breaker(BreakerNote::Tripped(BreakerTrip::Probing));
    let open = SessionEntry::Opened(opening());
    let paused = plan(&[open.clone(), tripped.clone()]);
    assert_eq!(paused.standing, Standing::Paused(BreakerTrip::Probing));
    let turned = plan(&[
        open.clone(),
        tripped.clone(),
        SessionEntry::Turn(turn(1, "ok")),
    ]);
    assert_eq!(turned.standing, Standing::Open);
    let reset = plan(&[open, tripped, SessionEntry::Breaker(BreakerNote::Reset)]);
    assert_eq!(reset.standing, Standing::Open);
}

#[test]
fn closed_stays_closed_with_its_first_cause() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        SessionEntry::Closed(EndCause::WallExhausted),
        SessionEntry::Turn(turn(1, "late")),
        SessionEntry::Closed(EndCause::Closed),
    ]);
    assert_eq!(p.standing, Standing::Closed(EndCause::WallExhausted));
}

#[test]
fn handles_come_back_as_labels_and_the_latest_wins() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        taint(),
        untrusted_handle(1),
        trusted_handle(1),
    ]);
    assert_eq!(p.handles.len(), 1);
    assert_eq!(p.handles[0].shape, docket_core::HandleShape::File);
}

#[test]
fn an_untrusted_handle_with_no_taint_before_it_is_a_fault_and_tainted() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        untrusted_handle(1),
        taint(),
    ]);
    assert_eq!(p.taint, Taint::Tainted);
    assert_eq!(p.faults, vec![ResumeFault::MissingTaint { at: Seq(1) }]);
}

#[test]
fn a_trusted_handle_needs_no_taint() {
    let p = plan(&[SessionEntry::Opened(opening()), trusted_handle(1)]);
    assert_eq!(p.taint, Taint::Clean);
    assert!(p.faults.is_empty());
}

#[test]
fn a_gap_blocks_and_taints() {
    let mut r = rows(&canonical()[..4]);
    r[3].seq = Seq(5);
    let p = resume_plan(&r).expect("plan");
    assert_eq!(p.standing, Standing::Blocked(Blocker::Gap));
    assert_eq!(p.taint, Taint::Tainted);
    assert_eq!(
        p.faults,
        vec![ResumeFault::Gap {
            expected: Seq(3),
            found: Seq(5)
        }]
    );
}

#[test]
fn a_second_opening_is_ignored_and_reported() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        SessionEntry::Opened(opening()),
    ]);
    assert_eq!(p.faults, vec![ResumeFault::DuplicateOpened { at: Seq(1) }]);
}

#[test]
fn skills_are_listed_once() {
    let skill = SessionEntry::Skill(SkillUse {
        id: docket_core::SkillId::parse("triage").expect("id"),
        version: docket_core::SkillVersion("0.1.0".into()),
    });
    let p = plan(&[SessionEntry::Opened(opening()), skill.clone(), skill]);
    assert_eq!(p.skills.len(), 1);
}

#[test]
fn a_budget_checkpoint_resets_the_calls_since() {
    let p = plan(&[
        SessionEntry::Opened(opening()),
        call(1, "mail.thread.search"),
        SessionEntry::Budget(ledger(1)),
        call(2, "mail.thread.search"),
    ]);
    assert_eq!(p.budget.calls_since.0, 1);
}
