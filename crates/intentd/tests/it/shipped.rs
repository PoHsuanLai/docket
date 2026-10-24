//! The search file an app ships: optional fields default, entries are in no Space and never
//! updated, and anything else in the file is refused.

use intentd::{ShippedEntries, ShippedFault};
use prov::{SpaceScope, UnixSeconds};

#[test]
fn a_file_gives_entries_with_defaults_for_what_it_leaves_out() {
    let text = r#"
[[entries]]
kind = "settings.page"
key = "wifi"
title = "Wi-Fi"
subtitle = "Join and forget networks"
keywords = ["wireless"]

[[entries]]
kind = "settings.page"
key = "about"
title = "About"
"#;
    let entries = ShippedEntries::parse(text).expect("parsed").into_entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].keywords, ["wireless"]);
    assert_eq!(entries[1].subtitle, "");
    assert!(entries[1].keywords.is_empty());
    assert_eq!(entries[1].updated, UnixSeconds(0));
    assert_eq!(entries[1].space, SpaceScope::Any);
}

#[test]
fn a_file_that_is_not_a_search_file_is_malformed() {
    for text in [
        "not toml at all",
        "[[entries]]\nkind = \"settings.page\"\n",
        "[[entries]]\nkind = \"Bad Kind\"\nkey = \"k\"\ntitle = \"T\"\n",
        "[[entries]]\nkind = \"a.b\"\nkey = \"k\"\ntitle = \"T\"\nextra = 1\n",
    ] {
        assert!(
            matches!(ShippedEntries::parse(text), Err(ShippedFault::Malformed(_))),
            "{text}"
        );
    }
}
