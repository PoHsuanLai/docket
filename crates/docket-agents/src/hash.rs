//! SHA-256 of an archive.

use sha2::{Digest, Sha256};

/// A SHA-256, as lower-case hex.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sha256Hex(String);

/// Why a digest was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("a SHA-256 is 64 hex digits")]
pub struct DigestRefused;

impl Sha256Hex {
    /// The digest of `bytes`.
    pub fn of(bytes: &[u8]) -> Self {
        let sum = Sha256::digest(bytes);
        Self(sum.iter().map(|b| format!("{b:02x}")).collect())
    }

    /// A digest the registry or a record wrote.
    pub fn parse(text: &str) -> Result<Self, DigestRefused> {
        let text = text.trim().to_ascii_lowercase();
        if text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(text))
        } else {
            Err(DigestRefused)
        }
    }

    /// The hex.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
