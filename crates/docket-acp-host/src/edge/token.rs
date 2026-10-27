//! Minting a session's token: 32 bytes from the kernel, as hex.

use actions_tools::EdgeToken;

/// A new token, from the kernel's random bytes.
pub(super) fn mint() -> std::io::Result<EdgeToken> {
    let mut bytes = [0_u8; 32];
    getrandom::fill(&mut bytes).map_err(std::io::Error::other)?;
    EdgeToken::parse(&const_hex::encode(bytes)).ok_or_else(|| std::io::Error::other("token"))
}

/// Eight random bytes as hex, to make a directory name that cannot be guessed.
pub(super) fn nonce() -> std::io::Result<String> {
    Ok(mint()?.reveal()[..16].to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_64_lower_case_hex_digits_and_a_nonce_is_16_of_them() {
        let token = mint().expect("token");
        let text = token.reveal();
        assert_eq!(text.len(), 64);
        assert!(text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')));
        assert_eq!(nonce().expect("nonce").len(), 16);
        assert_ne!(token.reveal(), mint().expect("another").reveal());
    }
}
