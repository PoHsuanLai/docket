//! Sample bytes.

/// S16LE bytes as samples (a trailing odd byte is dropped).
pub fn samples_of(bytes: &[u8]) -> Vec<i16> {
    bytes
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| i16::from_le_bytes(*b))
        .collect()
}
