//! The configuration.

use porter_core::AppName;
use std::path::PathBuf;
use voiced::*;

#[test]
fn the_shipped_voiced_toml_parses_and_round_trips() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../dist/voiced.toml");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let config = VoicedConfig::parse(&text).expect("parses");
    let shell = AppName::parse("org.quire.Shell").expect("app");
    assert_eq!(config.role_of(&shell), Some(VoiceRole::Shell));
    assert_eq!(
        config.role_of(&AppName::parse("org.quire.Mail").expect("app")),
        None
    );
    let again = VoicedConfig::parse(&toml::to_string(&config).expect("toml")).expect("again");
    assert_eq!(again, config);
}

#[test]
fn a_file_without_a_shell_is_refused() {
    assert_eq!(
        VoicedConfig::parse("earcons = \"on\"\n[roles]\n"),
        Err(ConfigError::NoShell)
    );
    assert!(matches!(
        VoicedConfig::parse("nonsense"),
        Err(ConfigError::Parse(_))
    ));
}

#[test]
fn the_input_override_is_optional_in_voiced_toml() {
    let base = "earcons = \"on\"\n";
    let roles = "[roles]\nshell = [\"org.quire.Shell\"]\n";
    let none = VoicedConfig::parse(&format!("{base}{roles}")).expect("parses");
    assert_eq!(none.input, None);
    let some = VoicedConfig::parse(&format!("{base}input = \"alsa_input.usb-mic\"\n{roles}"))
        .expect("parses");
    assert_eq!(some.input.as_deref(), Some("alsa_input.usb-mic"));
    let again = VoicedConfig::parse(&toml::to_string(&some).expect("toml")).expect("again");
    assert_eq!(again, some);
}
