//! The breaker: 3 consecutive, 10 of the last 50, or probing.

use action_review::*;
use docket_core::{AgentConfig, BreakerTrip};
use porter_core::{AppName, Count};
use prov::{ActionName, UnixSeconds};

fn goal(action: &str) -> GoalKey {
    GoalKey {
        app: AppName::parse("org.quire.Mail").expect("app"),
        action: ActionName::parse(action).expect("action"),
        kind: None,
    }
}

fn denied(action: &str, args: u64) -> DecisionMark {
    DecisionMark::Denied(DenialMark {
        at: UnixSeconds(0),
        goal: goal(action),
        args: ArgDigest(args),
        by: DeniedBy::Reviewer,
    })
}

fn run(decisions: Vec<DecisionMark>) -> (Breaker, Vec<Option<BreakerTrip>>) {
    let limits = AgentConfig::default().breaker;
    let mut breaker = Breaker::new();
    let mut trips = Vec::new();
    for d in decisions {
        let (next, trip) = note(breaker, d, &limits);
        breaker = next;
        trips.push(trip);
    }
    (breaker, trips)
}

#[test]
fn breaker_trips_on_three_consecutive() {
    let (_, trips) = run(vec![
        denied("mail.a.one", 1),
        denied("mail.b.two", 2),
        denied("mail.c.three", 3),
    ]);
    assert_eq!(trips, [None, None, Some(BreakerTrip::Consecutive)]);
}

#[test]
fn allow_resets_consecutive_not_recent() {
    let (breaker, trips) = run(vec![
        denied("mail.a.one", 1),
        denied("mail.b.two", 2),
        DecisionMark::Allowed,
        denied("mail.c.three", 3),
    ]);
    assert!(trips.iter().all(Option::is_none));
    assert_eq!(breaker.consecutive, Count(1));
    let denials = breaker
        .recent
        .iter()
        .filter(|d| matches!(d, DecisionMark::Denied(_)))
        .count();
    assert_eq!(denials, 3, "the allow did not clear the recent window");
}

#[test]
fn breaker_trips_on_ten_of_fifty() {
    // Nine denials spread by allows (never three in a row, never the same goal), then the tenth.
    let mut decisions = Vec::new();
    for i in 0..9u64 {
        decisions.push(denied(&format!("mail.thing.act{i}"), i));
        decisions.push(DecisionMark::Allowed);
    }
    decisions.push(denied("mail.thing.last", 99));
    let (_, trips) = run(decisions);
    assert!(
        trips[..trips.len() - 1].iter().all(Option::is_none),
        "{trips:?}"
    );
    assert_eq!(trips.last(), Some(&Some(BreakerTrip::Recent)));
}

#[test]
fn the_window_forgets_decisions_older_than_fifty() {
    let mut decisions = vec![denied("mail.thing.old", 1)];
    decisions.extend(std::iter::repeat_n(DecisionMark::Allowed, 50));
    let (breaker, _) = run(decisions);
    assert_eq!(breaker.recent.len(), 50);
    assert!(breaker.recent.iter().all(|d| *d == DecisionMark::Allowed));
}

#[test]
fn breaker_detects_probing() {
    // The same goal with different arguments, separated by allows so only probing can trip.
    let (_, trips) = run(vec![
        denied("mail.message.send", 1),
        DecisionMark::Allowed,
        denied("mail.message.send", 2),
        DecisionMark::Allowed,
        denied("mail.message.send", 3),
    ]);
    assert_eq!(trips.last(), Some(&Some(BreakerTrip::Probing)));
    assert!(trips[..4].iter().all(Option::is_none), "{trips:?}");
}

#[test]
fn the_same_arguments_three_times_is_a_repeat_not_probing() {
    let (breaker, trips) = run(vec![
        denied("mail.message.send", 7),
        DecisionMark::Allowed,
        denied("mail.message.send", 7),
        DecisionMark::Allowed,
        denied("mail.message.send", 7),
    ]);
    assert!(trips.iter().all(Option::is_none), "{trips:?}");
    assert_eq!(
        repeated(&breaker, &goal("mail.message.send"), ArgDigest(7)),
        RepeatState::Repeated
    );
    assert_eq!(
        repeated(&breaker, &goal("mail.message.send"), ArgDigest(8)),
        RepeatState::Fresh
    );
    assert_eq!(
        repeated(&breaker, &goal("mail.thread.read"), ArgDigest(7)),
        RepeatState::Fresh
    );
}

#[test]
fn a_resumed_session_starts_clean() {
    let (tripped, _) = run(vec![
        denied("mail.a.one", 1),
        denied("mail.b.two", 2),
        denied("mail.c.three", 3),
    ]);
    assert_ne!(tripped, Breaker::new());
    assert_eq!(Breaker::new().consecutive, Count(0));
    assert!(Breaker::default().recent.is_empty());
}
