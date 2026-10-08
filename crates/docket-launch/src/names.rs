//! The session name accountd sees. An ACP session id is whatever the agent or the host chose;
//! accountd takes the grammar `[a-z0-9][a-z0-9._-]*` of at most 64 bytes. Ours are mapped:
//! `acp-` and the id when that fits, else `acp-` and a 16-digit hex digest of the id. The same
//! id always maps to the same name, and accountd never sees the original.

use porter_core::LauncherSession;
use prov::SessionId;

const PREFIX: &str = "acp-";

fn fits(text: &str) -> bool {
    let mut bytes = text.bytes();
    bytes
        .next()
        .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        && text.len() <= 64
        && text.bytes().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'.' | b'_' | b'-')
        })
}

fn digest(text: &str) -> String {
    let hash = text.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    format!("{hash:016x}")
}

/// The launcher session for `session`.
pub fn launcher_session(session: &SessionId) -> Option<LauncherSession> {
    let whole = format!("{PREFIX}{}", session.as_str());
    let name = if fits(&whole) {
        whole
    } else {
        format!("{PREFIX}{}", digest(session.as_str()))
    };
    LauncherSession::parse(&name).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(text: &str) -> SessionId {
        SessionId::parse(text).expect("session")
    }

    #[test]
    fn a_fitting_id_keeps_its_text_and_others_are_hashed_the_same_way_each_time() {
        assert_eq!(
            launcher_session(&id("s-1")).expect("name").as_str(),
            "acp-s-1"
        );
        let long = "a".repeat(64);
        let mapped = launcher_session(&id(&long)).expect("name");
        assert_eq!(mapped.as_str().len(), 20);
        assert_eq!(Some(mapped), launcher_session(&id(&long)));
        assert_ne!(
            launcher_session(&id(&long)),
            launcher_session(&id(&"b".repeat(64)))
        );
    }
}
