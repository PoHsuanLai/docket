//! Who may restore a stored plan: the opener rule, and an editor in a helper's conversation.

use crate::support::*;
use docket_core::CallerRole;
use docket_core::helper::{HelperName, HelperSeat, SeatPlace};
use docket_session::*;
use prov::AppName;

fn plan_with(place: Option<SeatPlace>) -> ResumePlan {
    let seat = place.map(|place| {
        SessionEntry::Seat(HelperSeat {
            helper: serde_json::from_str("\"h-1\"").expect("helper id"),
            name: HelperName::parse("Inbox").expect("name"),
            reach: policy(),
            owns: Vec::new(),
            place,
        })
    });
    let entries: Vec<SessionEntry> = std::iter::once(SessionEntry::Opened(opening()))
        .chain(seat)
        .collect();
    resume_plan(&rows(&entries)).expect("a plan")
}

#[test]
fn an_editor_restores_a_helpers_conversation_and_nothing_else_it_did_not_open() {
    let editor = AppName::parse("org.zed.Zed").expect("app");
    let owner = mail();
    let table = [
        (
            CallerRole::Editor,
            &editor,
            Some(SeatPlace::Conversation),
            true,
            "an editor speaks in a conversation",
        ),
        (
            CallerRole::Editor,
            &editor,
            Some(SeatPlace::Job),
            false,
            "a job stays the companion's",
        ),
        (
            CallerRole::Editor,
            &editor,
            None,
            false,
            "an unseated session of another app",
        ),
        (
            CallerRole::App,
            &editor,
            Some(SeatPlace::Conversation),
            false,
            "an app does not",
        ),
        (
            CallerRole::Field,
            &editor,
            Some(SeatPlace::Conversation),
            false,
            "a field does not",
        ),
        (
            CallerRole::Editor,
            &owner,
            Some(SeatPlace::Job),
            true,
            "an editor still restores what its own app opened",
        ),
    ];
    for (role, app, place, want, why) in table {
        let claim = Claimant { role, app };
        assert_eq!(may_restore_plan(claim, &plan_with(place)), want, "{why}");
    }
}

#[test]
fn a_fork_rule_does_not_see_the_seat() {
    let editor = AppName::parse("org.zed.Zed").expect("app");
    let claim = Claimant {
        role: CallerRole::Editor,
        app: &editor,
    };
    let plan = plan_with(Some(SeatPlace::Conversation));
    assert!(!may_restore_opening(claim, &plan.opening));
}
