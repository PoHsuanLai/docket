//! Names, digests and platform keys.

use agents_core::dirs::AgentsDir;
use agents_core::hash::Sha256Hex;
use agents_core::platform::Platform;
use agents_core::slug::Slug;
use std::path::Path;

#[test]
fn names_that_could_climb_out_are_refused() {
    for bad in ["", ".", "..", "../x", ".hidden", "a/b", "a b"] {
        assert!(Slug::parse(bad).is_err(), "{bad:?}");
    }
    assert!(Slug::parse(&"x".repeat(65)).is_err());
    assert_eq!(
        Slug::parse("agy-acp_1.0+b").expect("slug").as_str(),
        "agy-acp_1.0+b"
    );
}

#[test]
fn a_digest_is_64_hex_digits_in_lower_case() {
    let of = Sha256Hex::of(b"abc");
    assert_eq!(
        of.as_str(),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let upper = of.as_str().to_ascii_uppercase();
    assert_eq!(Sha256Hex::parse(&format!(" {upper}\n")), Ok(of));
    assert!(Sha256Hex::parse("abc").is_err());
}

#[test]
fn the_platform_key_spells_macos_as_darwin() {
    assert_eq!(Platform::of("macos", "aarch64").as_str(), "darwin-aarch64");
    assert_eq!(
        Platform::of("linux", "x86_64"),
        Platform::named("linux-x86_64")
    );
}

#[test]
fn the_layout_is_paths_under_the_root() {
    let dir = AgentsDir::at(Path::new("/r"));
    let (id, version) = (
        Slug::parse("a").expect("id"),
        Slug::parse("1").expect("version"),
    );
    assert_eq!(dir.installed(&id, &version), Path::new("/r/installed/a/1"));
    assert_eq!(dir.pin(&id, &version), Path::new("/r/pins/a/1.sha256"));
    assert_eq!(dir.home(&id), Path::new("/r/state/a/home"));
    assert_eq!(dir.offered(&id), Path::new("/r/state/a/offered.toml"));
}
