//! PipeWire's default source, as the `default` metadata object states it. Two keys name it:
//! `default.configured.audio.source` (what the person picked in their sound settings) and
//! `default.audio.source` (what the session manager resolved). Both carry the JSON
//! `{"name": "<node.name>"}`; the configured one wins when present.

use serde::Deserialize;

/// The metadata key of the person's own pick.
pub const CONFIGURED_KEY: &str = "default.configured.audio.source";
/// The metadata key of the resolved default.
pub const RESOLVED_KEY: &str = "default.audio.source";

#[derive(Deserialize)]
struct NameValue {
    name: String,
}

/// The node name in one metadata value. A value that is not `{"name": "..."}`, or names nothing,
/// is no name; a bare string is taken as the name too, since older session managers wrote that.
pub fn name_in(value: &str) -> Option<String> {
    let name = match serde_json::from_str::<NameValue>(value) {
        Ok(parsed) => parsed.name,
        Err(_) => match serde_json::from_str::<String>(value) {
            Ok(bare) => bare,
            Err(_) => return None,
        },
    };
    Some(name).filter(|n| !n.is_empty())
}

/// What the `default` metadata object said about the source, key by key.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DefaultSources {
    configured: Option<String>,
    resolved: Option<String>,
}

impl DefaultSources {
    /// Takes one property update; `value` is none when the key was removed.
    pub fn note(&mut self, key: &str, value: Option<&str>) {
        let name = value.and_then(name_in);
        match key {
            CONFIGURED_KEY => self.configured = name,
            RESOLVED_KEY => self.resolved = name,
            _ => {}
        }
    }

    /// The default source's node name: the configured one if present, else the resolved one.
    pub fn name(&self) -> Option<&str> {
        self.configured.as_deref().or(self.resolved.as_deref())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_json_shapes_of_a_metadata_value() {
        let table = [
            (
                r#"{"name":"alsa_input.usb-mic"}"#,
                Some("alsa_input.usb-mic"),
            ),
            (r#"{ "name" : "x" , "extra": 3 }"#, Some("x")),
            (r#""alsa_input.bare""#, Some("alsa_input.bare")),
            (r#"{"name":""}"#, None),
            (r#"{"name":null}"#, None),
            (r#"{"id":4}"#, None),
            (r#"[]"#, None),
            ("not json", None),
            ("", None),
        ];
        for (text, want) in table {
            assert_eq!(name_in(text).as_deref(), want, "{text}");
        }
    }

    #[test]
    fn the_configured_key_beats_the_resolved_one_and_removal_falls_back() {
        let mut seen = DefaultSources::default();
        assert_eq!(seen.name(), None);
        seen.note(RESOLVED_KEY, Some(r#"{"name":"resolved"}"#));
        assert_eq!(seen.name(), Some("resolved"));
        seen.note(CONFIGURED_KEY, Some(r#"{"name":"configured"}"#));
        assert_eq!(seen.name(), Some("configured"));
        seen.note("default.audio.sink", Some(r#"{"name":"sink"}"#));
        assert_eq!(seen.name(), Some("configured"));
        seen.note(CONFIGURED_KEY, None);
        assert_eq!(seen.name(), Some("resolved"));
        seen.note(RESOLVED_KEY, Some("garbage"));
        assert_eq!(seen.name(), None);
    }
}
