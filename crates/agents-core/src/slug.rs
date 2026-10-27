//! A registry id or version that is safe to use as a directory name.

/// 1 to 64 letters, digits, `-`, `_`, `.` and `+`, not starting with a dot.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slug(String);

/// Why a name was not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0:?} is not a name docket can keep an agent under")]
pub struct SlugRefused(String);

impl Slug {
    /// Checks `text`.
    pub fn parse(text: &str) -> Result<Self, SlugRefused> {
        let fine = !text.is_empty()
            && text.len() <= 64
            && !text.starts_with('.')
            && text
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'+'));
        if fine {
            Ok(Self(text.to_owned()))
        } else {
            Err(SlugRefused(text.to_owned()))
        }
    }

    /// The text.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
