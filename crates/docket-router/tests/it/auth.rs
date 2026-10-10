//! Who may call which member of Intents1.

use docket_core::*;
use docket_router::*;

fn role_set(member: Member) -> Vec<CallerRole> {
    CallerRole::ALL
        .into_iter()
        .filter(|r| permits(*r, member))
        .collect()
}

#[test]
fn only_the_persons_surfaces_record_a_turn_and_the_same_roles_end_it() {
    for member in [Member::SessionTurn, Member::SessionTurnEnded] {
        assert_eq!(
            role_set(member),
            [
                CallerRole::Launcher,
                CallerRole::Field,
                CallerRole::Editor,
                CallerRole::Cli,
                CallerRole::AcpAgent
            ],
            "{member:?}"
        );
        assert!(!permits(CallerRole::Mcp, member), "{member:?}");
    }
}

#[test]
fn the_host_of_an_external_agent_opens_speaks_and_performs_and_nothing_else() {
    let allowed: Vec<Member> = Member::ALL
        .into_iter()
        .filter(|m| permits(CallerRole::AcpAgent, *m))
        .collect();
    assert_eq!(
        allowed,
        [
            Member::Manifests,
            Member::Perform,
            Member::SessionOpen,
            Member::SessionTurn,
            Member::SessionClose,
            Member::SessionTurnEnded
        ]
    );
}

#[test]
fn an_editor_opens_speaks_and_reads_the_log_but_confirms_halts_and_performs_nothing() {
    let allowed: Vec<Member> = Member::ALL
        .into_iter()
        .filter(|m| permits(CallerRole::Editor, *m))
        .collect();
    assert_eq!(
        allowed,
        [
            Member::Manifests,
            Member::SessionOpen,
            Member::SessionTurn,
            Member::SessionClose,
            Member::SessionTurnEnded,
            Member::SessionStored,
            Member::CheckpointList,
            Member::CheckpointPlan
        ]
    );
}

#[test]
fn restore_points_are_for_the_persons_surfaces_and_never_an_agent() {
    for member in [Member::CheckpointList, Member::CheckpointPlan] {
        assert_eq!(
            role_set(member),
            [
                CallerRole::Launcher,
                CallerRole::Field,
                CallerRole::Editor,
                CallerRole::Companion,
                CallerRole::Cli,
                CallerRole::Control
            ],
            "{member:?}"
        );
        assert!(!permits(CallerRole::Mcp, member), "{member:?}");
        assert!(!permits(CallerRole::AcpAgent, member), "{member:?}");
    }
}

#[test]
fn only_a_terminal_watches_an_agent_and_marks_its_turns() {
    for member in [Member::CheckpointWatch, Member::CheckpointMark] {
        assert_eq!(role_set(member), [CallerRole::Cli], "{member:?}");
    }
}

#[test]
fn only_the_reader_resolves_a_handle_and_never_a_planner() {
    assert_eq!(role_set(Member::SessionResolve), [CallerRole::Reader]);
    assert!(!permits(CallerRole::Companion, Member::SessionResolve));
    // Display is for the screen: a planner never gets the text.
    assert!(!permits(CallerRole::Companion, Member::SessionDisplay));
}

#[test]
fn only_cuad_uses_the_gate_and_only_control_resumes() {
    assert_eq!(role_set(Member::GateCheck), [CallerRole::Cua]);
    assert_eq!(role_set(Member::GateGrant), [CallerRole::Cua]);
    assert_eq!(role_set(Member::ControlResume), [CallerRole::Control]);
    assert_eq!(
        role_set(Member::ControlHalt),
        [CallerRole::Compositor, CallerRole::Control]
    );
}

#[test]
fn nobody_but_the_planner_reads_memory_or_notes_an_episode() {
    for member in [
        Member::SessionRecall,
        Member::SessionNote,
        Member::SessionRead,
    ] {
        assert_eq!(role_set(member), [CallerRole::Companion], "{member:?}");
    }
}

#[test]
fn a_run_may_send_a_report_but_a_reader_may_send_nothing() {
    assert!(permits(CallerRole::Cua, Member::MessageSend));
    assert!(permits(CallerRole::Launcher, Member::MessageSend));
    for member in Member::ALL {
        if member != Member::Manifests {
            assert!(
                !permits(CallerRole::Reader, member) || member == Member::SessionResolve,
                "{member:?}"
            );
        }
    }
}

#[test]
fn every_member_is_reachable_by_some_role_and_the_registry_by_all() {
    for member in Member::ALL {
        assert!(!role_set(member).is_empty(), "{member:?} has no caller");
    }
    assert_eq!(role_set(Member::Manifests).len(), CallerRole::ALL.len());
}

#[test]
fn a_caller_acts_in_the_first_of_its_roles_that_may_make_the_call() {
    use std::collections::BTreeSet;
    let sill = BTreeSet::from([
        CallerRole::Launcher,
        CallerRole::Confirm,
        CallerRole::Control,
    ]);
    assert_eq!(
        acting_role(&sill, Member::Perform),
        Some(CallerRole::Launcher),
        "sill performs as the person"
    );
    assert_eq!(
        acting_role(&sill, Member::ControlResume),
        Some(CallerRole::Control)
    );
    assert_eq!(
        acting_role(&sill, Member::GateCheck),
        None,
        "sill is not cuad"
    );
    let plain = BTreeSet::new();
    assert_eq!(
        acting_role(&plain, Member::SearchQuery),
        Some(CallerRole::App)
    );
    assert_eq!(
        acting_role(&plain, Member::ControlHalt),
        None,
        "a plain app cannot halt"
    );
    assert_eq!(
        acting_role(&plain, Member::SessionTurn),
        None,
        "and cannot record a turn"
    );
}

#[test]
fn a_terminal_gets_exactly_the_members_quire_do_uses() {
    let cli: Vec<Member> = Member::ALL
        .into_iter()
        .filter(|m| permits(CallerRole::Cli, *m))
        .collect();
    assert_eq!(
        cli,
        [
            Member::Manifests,
            Member::SearchQuery,
            Member::SearchCancel,
            Member::Perform,
            Member::DryRun,
            Member::Undo,
            Member::Context,
            Member::SessionTurn,
            Member::SessionClose,
            Member::SessionTurnEnded,
            Member::SessionStored,
            // The terminal shows its own sessions' restore points (owner, 2026-10-10).
            Member::CheckpointList,
            Member::CheckpointPlan,
            // It watches the agents in its panes (never hosts them).
            Member::CheckpointWatch,
            Member::CheckpointMark,
            Member::ControlJournal,
        ]
    );
}

#[test]
fn only_the_control_centre_shows_and_revokes_what_the_terminal_may_do_unasked() {
    assert_eq!(
        role_set(Member::ControlTerminalGrants),
        [CallerRole::Control]
    );
    assert_eq!(
        role_set(Member::ControlTerminalRevoke),
        [CallerRole::Control]
    );
}
