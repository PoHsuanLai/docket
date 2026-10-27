//! Minting a session's token: 32 bytes from the kernel, as hex.

use actions_tools::EdgeToken;
use std::io::Read;

/// A new token, from `/dev/urandom`.
pub(super) fn mint() -> std::io::Result<EdgeToken> {
    let mut bytes = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    EdgeToken::parse(&hex).ok_or_else(|| std::io::Error::other("token"))
}

/// Eight random bytes as hex, to make a directory name that cannot be guessed.
pub(super) fn nonce() -> std::io::Result<String> {
    Ok(mint()?.reveal()[..16].to_owned())
}
