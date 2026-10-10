//! Who may call which member of `Intents1`: a table from member to roles, pure.

use docket_core::{CallerRole, Member};

use CallerRole::{
    AcpAgent, App, Cli, Companion, Compositor, Control, Cua, Editor, Field, Launcher, Mcp, Reader,
};

/// The roles that may call `member`.
fn roles(member: Member) -> &'static [CallerRole] {
    match member {
        Member::Manifests => &CallerRole::ALL,
        // The owning app only; the router checks the name equals the manifest's app.
        Member::IndexPush | Member::IndexReset => &[App],
        Member::SearchQuery | Member::SearchCancel => &[Launcher, Companion, Cli, App],
        // The person's surfaces, the planner, MCP, a terminal, and an app calling its own
        // actions. cuad asks through the gate, not through Perform; the reader acts on nothing.
        Member::Perform => &[Launcher, Field, Companion, Mcp, Cli, App, AcpAgent],
        Member::DryRun => &[Launcher, Field, Companion, Mcp, Cli, App],
        Member::Preview | Member::Suggest => &[Launcher, Field, Companion, Mcp, App],
        // A terminal undoes only what a terminal did (`run_undo` checks the journal row).
        Member::Undo => &[Launcher, Field, Control, Cli, App],
        Member::UndoAll => &[Launcher, Field, Control, App],
        Member::Context => &[Launcher, Companion, Cli],
        // The computer-use daemon opens and closes the session of its own run (`opening.rs`
        // refuses it any other kind), so the run is ruled in a session before its first step.
        Member::SessionOpen | Member::SessionClose => {
            &[Launcher, Field, Editor, Companion, Cua, AcpAgent]
        }
        // Only the person's own surfaces record a turn: a model cannot say what the person said.
        // A terminal is one of them for `quire-do ask` (the person at a keyboard, the same user),
        // and its turns are recorded as `TurnSource::Terminal`, never as the launcher's.
        Member::SessionTurn => &[Launcher, Field, Editor, Cli, AcpAgent],
        // The host that recorded a turn says when it ended: exactly the roles of `Session.Turn`,
        // `AcpAgent` included (it is the role docket's external-agent host runs as, and it
        // records turns), never `Mcp`. Safe: it can only end the running-turn window; a restore
        // still needs the person's confirmation on the sheet.
        Member::SessionTurnEnded => &[Launcher, Field, Editor, Cli, AcpAgent],
        Member::SessionResolve => &[Reader],
        // Text for the screen, never for a model.
        Member::SessionDisplay | Member::SessionDisplayLabelled => &[Launcher, Field],
        Member::SessionRead
        | Member::SessionNote
        | Member::SessionRecall
        | Member::SessionNarrow
        | Member::SessionHandles => &[Companion],
        // The durable log, for the edges that list, load and fork sessions; the router lists only
        // what the caller may bring back (a terminal: what a terminal opened).
        Member::SessionStored => &[Launcher, Editor, Companion, Cli],
        // A session's restore points and what restoring one would change: the person's own
        // surfaces only, never an agent (Mcp, AcpAgent): it cannot read or restore its own.
        Member::CheckpointList | Member::CheckpointPlan => {
            &[Launcher, Field, Editor, Companion, Control, Cli]
        }
        // A terminal watching an agent it does not host: it opens a restore-point-only session
        // and marks the agent's turns. Never `Mcp` or `AcpAgent`; the router lets only the
        // opener use the session (`watched.rs`).
        Member::CheckpointWatch | Member::CheckpointMark => &[Cli],
        Member::SessionTaskPolicy | Member::SessionWiden => &[Launcher, Field, Companion],
        // The person talks to a subagent through the launcher; agents and runs report.
        Member::MessageSend => &[Launcher, Field, Companion, Cua],
        Member::MessageInbox => &[Launcher, Companion, Cua],
        Member::GateGrant | Member::GateCheck => &[Cua],
        Member::ControlHalt | Member::ControlState => &[Compositor, Control],
        Member::ControlResume => &[Control],
        // A terminal reads only its own rows (`control_journal` cuts the list).
        Member::ControlJournal => &[Launcher, Control, Cli],
        // The control centre shows and revokes what the terminal may do without asking.
        Member::ControlTerminalGrants
        | Member::ControlTerminalRevoke
        | Member::ControlStandingGrants
        | Member::ControlStandingRevoke => &[Control],
    }
}

/// Whether a caller in `role` may call `member`.
pub fn permits(role: CallerRole, member: Member) -> bool {
    roles(member).contains(&role)
}

/// The role `roles` acts in when it calls `member`: the first of its roles that may make the
/// call, or `App` for a name with no role of its own that may use the app-level members. None
/// when no role of the caller may make the call.
pub fn acting_role(
    roles: &std::collections::BTreeSet<CallerRole>,
    member: Member,
) -> Option<CallerRole> {
    let configured = roles.iter().copied().find(|r| permits(*r, member));
    configured.or_else(|| (roles.is_empty() && permits(App, member)).then_some(App))
}
