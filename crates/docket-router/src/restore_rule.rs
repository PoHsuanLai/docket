//! Who may bring a stored session back by naming it. Pure: the restore reads the log first, then
//! asks this with the opener the log records.
//!
//! A restored session carries its stored policy and handles, so naming one is a claim on them.
//! The rule is never looser than the one for a live session (`opening.rs`: a prompt field or the
//! computer-use daemon touches only the session its own app opened).

use docket_core::CallerRole;
use prov::AppName;

/// A caller asking for a stored session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Claimant<'a> {
    /// The role the request is made in.
    pub role: CallerRole,
    /// The app behind the connection.
    pub app: &'a AppName,
}

/// What the caller's role says before the opener is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Standing {
    /// The person's shell, or the companion: acts on any session, as it does live.
    Any,
    /// Every other role: only a session its own app opened.
    Own,
}

fn standing(role: CallerRole) -> Standing {
    match role {
        CallerRole::Launcher | CallerRole::Companion => Standing::Any,
        CallerRole::Field
        | CallerRole::Reader
        | CallerRole::Cua
        | CallerRole::Mcp
        | CallerRole::Cli
        | CallerRole::Confirm
        | CallerRole::Compositor
        | CallerRole::Control
        | CallerRole::App => Standing::Own,
    }
}

/// Whether `claimant` may restore a session opened by `opener` (none for a legacy log that
/// records no opener, which only the shell or the companion restores).
pub(crate) fn may_restore(claimant: Claimant<'_>, opener: Option<&AppName>) -> bool {
    match (standing(claimant.role), opener) {
        (Standing::Any, _) => true,
        (Standing::Own, Some(opened)) => opened == claimant.app,
        (Standing::Own, None) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app(s: &str) -> AppName {
        AppName::parse(s).expect("app")
    }

    #[test]
    fn table() {
        let a = app("org.quire.A");
        let b = app("org.quire.B");
        let shell = app("org.quire.Shell");
        let cases: [(CallerRole, &AppName, Option<&AppName>, bool, &str); 14] = [
            (CallerRole::Field, &a, Some(&a), true, "the opener restores"),
            (
                CallerRole::App,
                &a,
                Some(&a),
                true,
                "an app restores its own",
            ),
            (
                CallerRole::Cli,
                &a,
                Some(&a),
                true,
                "a terminal restores its own",
            ),
            (
                CallerRole::Field,
                &b,
                Some(&a),
                false,
                "another app gets unknown",
            ),
            (
                CallerRole::App,
                &b,
                Some(&a),
                false,
                "another app gets unknown",
            ),
            (
                CallerRole::Mcp,
                &b,
                Some(&a),
                false,
                "another client gets unknown",
            ),
            (
                CallerRole::Cua,
                &b,
                Some(&a),
                false,
                "cua cannot restore another's",
            ),
            (
                CallerRole::Cua,
                &a,
                Some(&a),
                true,
                "cua restores its own run",
            ),
            (
                CallerRole::Reader,
                &b,
                Some(&a),
                false,
                "the reader restores nothing alone",
            ),
            (
                CallerRole::Launcher,
                &shell,
                Some(&a),
                true,
                "the shell restores any",
            ),
            (
                CallerRole::Companion,
                &b,
                Some(&a),
                true,
                "the companion restores any",
            ),
            (
                CallerRole::Field,
                &a,
                None,
                false,
                "legacy: a field does not",
            ),
            (CallerRole::App, &a, None, false, "legacy: an app does not"),
            (
                CallerRole::Launcher,
                &shell,
                None,
                true,
                "legacy: the shell does",
            ),
        ];
        for (role, who, opener, want, why) in cases {
            let got = may_restore(Claimant { role, app: who }, opener);
            assert_eq!(got, want, "{why}");
        }
    }

    #[test]
    fn legacy_companion_restores() {
        let c = app("org.quire.Companiond");
        let claim = Claimant {
            role: CallerRole::Companion,
            app: &c,
        };
        assert!(may_restore(claim, None));
    }
}
