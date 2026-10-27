//! The platform keys the registry uses for a binary: `linux-x86_64`, `darwin-aarch64`, ...

/// A platform as the registry spells it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Deserialize)]
#[serde(transparent)]
pub struct Platform(String);

impl Platform {
    /// The platform of an operating system and architecture as Rust names them
    /// (`std::env::consts::OS` and `ARCH`): `macos` is `darwin` in the registry.
    pub fn of(os: &str, arch: &str) -> Self {
        let os = match os {
            "macos" => "darwin",
            other => other,
        };
        Self(format!("{os}-{arch}"))
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
