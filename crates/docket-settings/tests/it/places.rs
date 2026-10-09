//! The places setting: floor beats settings, order, no allowed place, ask, and never widening.

use docket_settings::*;

fn name(n: &str) -> PlaceName {
    PlaceName(n.to_owned())
}

fn own(n: &str, allowed: Toggle) -> OwnComputer {
    OwnComputer {
        name: name(n),
        allowed,
        model: None,
    }
}

fn cloud(n: &str, allowed: CloudAllowed) -> CloudAccount {
    CloudAccount {
        name: name(n),
        provider: "Acme".to_owned(),
        allowed,
        model: None,
    }
}

fn all_on() -> Places {
    Places {
        this_computer: None,
        own: vec![own("lab", Toggle::On)],
        cloud: vec![cloud("acme", CloudAllowed::On)],
    }
}

fn anywhere(_: &Place) -> bool {
    true
}

fn chosen_kind(r: Routed) -> PlaceKind {
    match r {
        Routed::Chosen(p) => p.kind(),
        other => panic!("expected Chosen, got {other:?}"),
    }
}

#[test]
fn local_then_own_then_cloud() {
    let places = all_on();
    assert_eq!(
        chosen_kind(route(Floor(PlaceKind::Cloud), &places, anywhere)),
        PlaceKind::ThisComputer
    );
    let not_here = |p: &Place| p.kind() != PlaceKind::ThisComputer;
    assert_eq!(
        chosen_kind(route(Floor(PlaceKind::Cloud), &places, not_here)),
        PlaceKind::OwnComputer
    );
    let only_cloud = |p: &Place| p.kind() == PlaceKind::Cloud;
    assert_eq!(
        chosen_kind(route(Floor(PlaceKind::Cloud), &places, only_cloud)),
        PlaceKind::Cloud
    );
}

#[test]
fn the_floor_beats_the_settings() {
    let places = all_on();
    let only_cloud = |p: &Place| p.kind() == PlaceKind::Cloud;
    assert_eq!(
        route(Floor(PlaceKind::OwnComputer), &places, only_cloud),
        Routed::Blocked
    );
    assert_eq!(
        route(Floor(PlaceKind::ThisComputer), &places, |p| p.kind()
            != PlaceKind::ThisComputer),
        Routed::Blocked
    );
}

#[test]
fn an_off_place_is_offered_never_used() {
    let places = Places {
        cloud: vec![cloud("acme", CloudAllowed::Off)],
        ..Places::none()
    };
    let only_cloud = |p: &Place| p.kind() == PlaceKind::Cloud;
    assert_eq!(
        route(Floor(PlaceKind::Cloud), &places, only_cloud),
        Routed::NoAllowedPlace {
            would_need: PlaceKind::Cloud
        }
    );
    let own_off = Places {
        own: vec![own("lab", Toggle::Off)],
        ..places
    };
    assert_eq!(
        route(Floor(PlaceKind::Cloud), &own_off, |p| p.kind()
            != PlaceKind::ThisComputer),
        Routed::NoAllowedPlace {
            would_need: PlaceKind::OwnComputer
        }
    );
}

#[test]
fn ask_each_time_asks() {
    let places = Places {
        cloud: vec![cloud("acme", CloudAllowed::AskEachTime)],
        ..Places::none()
    };
    let r = route(Floor(PlaceKind::Cloud), &places, |p| {
        p.kind() == PlaceKind::Cloud
    });
    assert!(matches!(r, Routed::Ask(Place::Cloud(_))));
}

#[test]
fn round_trip_and_bad_values_are_off() {
    let places = Places {
        this_computer: Some(ModelChoice("m".to_owned())),
        ..all_on()
    };
    assert_eq!(read_places(&write_places(&places)).places, places);
    let text = "[[assistant.own_computers]]\nname = \"lab\"\nallowed = \"ask\"\n\
                [[assistant.cloud_accounts]]\nname = \"a\"\n";
    let loaded = read_places(text);
    assert_eq!(loaded.places.own[0].allowed, Toggle::Off);
    assert_eq!(loaded.places.cloud[0].allowed, CloudAllowed::Off);
    assert_eq!(loaded.fallbacks.len(), 2);
}

#[test]
fn migration_never_widens() {
    let known = Known(vec![
        KnownPlace::OwnComputer(name("lab")),
        KnownPlace::Cloud(name("acme"), "Acme".to_owned()),
    ]);
    let migrated = Places::migrated("[ai]\nlocal_only = \"off\"\n", &known);
    assert_eq!(migrated.own[0].allowed, Toggle::Off);
    assert_eq!(migrated.cloud[0].allowed, CloudAllowed::Off);
    let set = Places {
        cloud: vec![cloud("acme", CloudAllowed::On)],
        ..Places::none()
    };
    let merged = set.merged(&known);
    assert_eq!(merged.cloud[0].allowed, CloudAllowed::On);
    assert_eq!(merged.own[0].allowed, Toggle::Off);
}

#[test]
fn the_places_table_is_not_an_unknown_key() {
    let text = write_places(&all_on());
    assert!(read(&text, AgentSettings::default()).unknown.is_empty());
}

#[test]
fn words_are_plain() {
    assert_eq!(kind_label(PlaceKind::OwnComputer), "Your computers");
    let s = confirm_turn_on(&name("me@acme"), "Acme");
    assert_eq!(
        s,
        "Turn on me@acme? Requests the assistant can't do on your computers may be sent to Acme."
    );
    for k in [
        PlaceKind::ThisComputer,
        PlaceKind::OwnComputer,
        PlaceKind::Cloud,
    ] {
        let said = no_place_says(k);
        assert!(!said.contains("MCP") && !said.contains("model"));
    }
}
