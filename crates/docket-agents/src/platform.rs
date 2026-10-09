//! The platform keys the registry uses for a binary: `linux-x86_64`, `darwin-aarch64`, ...

/// A platform as the registry spells it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(transparent)]
pub struct Platform(String);

impl Platform {
    /// The platform this build runs on.
    pub fn current() -> Self {
        let os = match std::env::consts::OS {
            "macos" => "darwin",
            other => other,
        };
        Self(format!("{os}-{}", std::env::consts::ARCH))
    }

    /// A platform by its registry key.
    pub fn named(key: &str) -> Self {
        Self(key.to_owned())
    }

    /// The registry key.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
