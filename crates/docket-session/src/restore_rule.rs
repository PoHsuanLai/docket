//! Who may bring a stored session back by naming it. Pure: the restore reads the log first, then
//! asks this with the opener the log records. The ACP edge asks it too, for `session/load` and
//! `session/list`.
//!
//! A restored session carries its stored policy and handles, so naming one is a claim on them.
//! The rule is never looser than the one for a live session (`opening.rs`: a prompt field or the
//! computer-use daemon touches only the session its own app opened).

use crate::Opening;
use docket_core::{CallerRole, StartedFrom};
use prov::AppName;

/// A caller asking for a stored session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Claimant<'a> {
    /// The role the request is made in.
    pub role: CallerRole,
    /// The app behind the connection.
    pub app: &'a AppName,
}

/// What the caller's role says before the opener is looked at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// The person's shell, or the companion: acts on any session, as it does live.
    Any,
    /// Every other role: only a session its own app opened.
    Own,
}

fn reach(role: CallerRole) -> Reach {
    match role {
        CallerRole::Launcher | CallerRole::Companion => Reach::Any,
        CallerRole::Field
        | CallerRole::Editor
        | CallerRole::Reader
        | CallerRole::Cua
        | CallerRole::Mcp
        | CallerRole::Cli
        | CallerRole::Confirm
        | CallerRole::Compositor
        | CallerRole::Control
        | CallerRole::AcpAgent
        | CallerRole::App => Reach::Own,
    }
}

/// Whether `claimant` may restore a session opened by `opener` (none for a legacy log that
/// records no opener, which only the shell or the companion restores).
pub fn may_restore(claimant: Claimant<'_>, opener: Option<&AppName>) -> bool {
    match (reach(claimant.role), opener) {
        (Reach::Any, _) => true,
        (Reach::Own, Some(opened)) => opened == claimant.app,
        (Reach::Own, None) => false,
    }
}

/// [`may_restore`] for a whole opening: the opener rule, and for a terminal (the `cli` role) also
/// a conversation a terminal started through the companion. Any terminal sees any such
/// conversation, not only its own scope: terminal scopes are advisory and every one runs as the
/// person. An editor, an app and the rest still see only what their own app opened.
pub fn may_restore_opening(claimant: Claimant<'_>, opening: &Opening) -> bool {
    may_restore(claimant, opening.opener.as_ref())
        || (claimant.role == CallerRole::Cli
            && matches!(opening.started_from, Some(StartedFrom::Terminal(_))))
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
        let cases: [(CallerRole, &AppName, Option<&AppName>, bool, &str); 17] = [
            (
                CallerRole::Editor,
                &a,
                Some(&a),
                true,
                "an editor restores what it opened",
            ),
            (
                CallerRole::Editor,
                &b,
                Some(&a),
                false,
                "an editor does not restore another's",
            ),
            (
                CallerRole::Editor,
                &a,
                None,
                false,
                "legacy: an editor does not",
            ),
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

    fn opening(opener: &AppName, started_from: Option<StartedFrom>) -> Opening {
        Opening {
            task: prov::TaskId::parse("t-1").expect("task"),
            space: prov::SpaceId::desktop(),
            opener: Some(opener.clone()),
            agent: None,
            backend: crate::BackendKind::Native,
            parent: None,
            forked_from: None,
            cwd: None,
            started_from,
        }
    }

    #[test]
    fn a_terminal_also_sees_what_a_terminal_started_and_nobody_else_does() {
        let (companion, a) = (app("org.quire.Companiond"), app("org.quire.A"));
        let from = docket_core::TerminalScope::from_cgroup("vte-spawn-1.scope").expect("scope");
        let started = opening(&companion, Some(StartedFrom::Terminal(from)));
        let plain = opening(&companion, None);
        let table = [
            (
                CallerRole::Cli,
                &started,
                true,
                "a terminal sees a terminal's",
            ),
            (
                CallerRole::Cli,
                &plain,
                false,
                "not a launcher's companion session",
            ),
            (CallerRole::Editor, &started, false, "an editor does not"),
            (CallerRole::App, &started, false, "an app does not"),
            (CallerRole::Field, &started, false, "a field does not"),
            (CallerRole::Mcp, &started, false, "an mcp client does not"),
        ];
        for (role, opening, want, why) in table {
            let claim = Claimant { role, app: &a };
            assert_eq!(may_restore_opening(claim, opening), want, "{why}");
        }
    }
}
