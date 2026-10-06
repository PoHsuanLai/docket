//! Whether voice may be used right now (`VoiceUse`): the person's choice, read when a call comes
//! and never cached, so a change in Settings takes effect on the next hold. The shipped source
//! reads sill's `settings.toml` (the `voice` table: `consent`, `hold_to_talk`); a test hands in
//! a fixed answer. An unreadable or unparseable file is `NeedsConsent`: voice never turns on by
//! accident.

use std::path::PathBuf;
use voice_wire::VoiceUse;

/// Where the answer comes from.
pub trait UseSource: Send + Sync + 'static {
    /// The answer now.
    fn now(&self) -> VoiceUse;
}

/// A fixed answer.
#[derive(Debug, Clone, Copy)]
pub struct FixedUse(pub VoiceUse);

impl UseSource for FixedUse {
    fn now(&self) -> VoiceUse {
        self.0
    }
}

/// The answer in a settings file.
#[derive(Debug, Clone)]
pub struct FileUse {
    path: PathBuf,
}

impl FileUse {
    /// Reads `path` on every call.
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// `$XDG_CONFIG_HOME/sill/settings.toml`, or `~/.config/sill/settings.toml`.
    pub fn sill_default() -> Self {
        let base = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .unwrap_or_default();
        Self::new(base.join("sill/settings.toml"))
    }
}

impl UseSource for FileUse {
    fn now(&self) -> VoiceUse {
        std::fs::read_to_string(&self.path)
            .map_or(VoiceUse::NeedsConsent, |text| use_of_settings(&text))
    }
}

/// What a settings text says: `hold_to_talk = "off"` is `Off`; `consent` of `"given"` (or a table
/// with a `given` key) is `On`, `"declined"` is `Off`, anything else `NeedsConsent`.
pub fn use_of_settings(text: &str) -> VoiceUse {
    let Ok(value) = text.parse::<toml::Table>() else {
        return VoiceUse::NeedsConsent;
    };
    let Some(voice) = value.get("voice").and_then(toml::Value::as_table) else {
        return VoiceUse::NeedsConsent;
    };
    if voice.get("hold_to_talk").and_then(toml::Value::as_str) == Some("off") {
        return VoiceUse::Off;
    }
    match voice.get("consent") {
        Some(toml::Value::String(s)) if s == "given" => VoiceUse::On,
        Some(toml::Value::String(s)) if s == "declined" => VoiceUse::Off,
        Some(toml::Value::Table(t)) if t.contains_key("given") => VoiceUse::On,
        _ => VoiceUse::NeedsConsent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_settings_text_decides_the_use() {
        let table = [
            ("", VoiceUse::NeedsConsent),
            ("not toml [", VoiceUse::NeedsConsent),
            ("[voice]\n", VoiceUse::NeedsConsent),
            ("[voice]\nconsent = \"not_asked\"\n", VoiceUse::NeedsConsent),
            ("[voice]\nconsent = \"given\"\n", VoiceUse::On),
            ("[voice.consent]\ngiven = 1760000000\n", VoiceUse::On),
            ("[voice]\nconsent = \"declined\"\n", VoiceUse::Off),
            (
                "[voice]\nconsent = \"given\"\nhold_to_talk = \"off\"\n",
                VoiceUse::Off,
            ),
        ];
        for (text, want) in table {
            assert_eq!(use_of_settings(text), want, "{text:?}");
        }
    }

    #[test]
    fn a_missing_file_needs_consent() {
        let source = FileUse::new(PathBuf::from("/nonexistent/sill/settings.toml"));
        assert_eq!(source.now(), VoiceUse::NeedsConsent);
    }
}
