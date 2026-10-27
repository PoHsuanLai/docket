//! Watching the almanac: when to ask again, and which events were already acted on.

use almanac_core::{EventRef, ReplicaId, Seq};
use companion_core::EventWatch;
use prov::{SpaceId, UnixSeconds};

fn event(seq: u64) -> EventRef {
    EventRef {
        space: SpaceId::parse("work").expect("space"),
        replica: ReplicaId([3; 16]),
        seq: Seq(seq),
    }
}

#[test]
fn the_almanac_is_asked_every_half_minute_from_the_last_question() {
    let mut watch = EventWatch::new(UnixSeconds(1_000));
    assert_eq!(watch.due(UnixSeconds(1_029)), None);
    assert_eq!(watch.due(UnixSeconds(1_030)), Some(UnixSeconds(1_000)));
    assert_eq!(watch.due(UnixSeconds(1_040)), None);
    assert_eq!(watch.due(UnixSeconds(1_060)), Some(UnixSeconds(1_030)));
}

#[test]
fn a_restart_does_not_act_on_events_from_before() {
    let mut watch = EventWatch::new(UnixSeconds(0));
    watch.restart(UnixSeconds(5_000));
    assert_eq!(watch.due(UnixSeconds(5_030)), Some(UnixSeconds(5_000)));
}

#[test]
fn an_event_is_acted_on_once() {
    let mut watch = EventWatch::new(UnixSeconds(0));
    assert!(watch.remember(&event(1)));
    assert!(!watch.remember(&event(1)));
    assert!(watch.remember(&event(2)));
}

#[test]
fn only_the_newest_events_are_remembered() {
    let mut watch = EventWatch::new(UnixSeconds(0));
    for seq in 0..600 {
        assert!(watch.remember(&event(seq)));
    }
    assert!(!watch.remember(&event(599)));
    assert!(watch.remember(&event(0)), "the oldest was let go");
}
