//! A helper's conversation with its agent app moving between docket and the app's own
//! command-line program in a terminal: where each conversation is, what the person can do about
//! it, and which conversations a terminal started that they can bring in. While a conversation is
//! in the terminal its helper is paused, so no job reaches the app.

use companion_model::WatchedLine;
use companion_model::helper::{Board, HandFault, HelperCard, HelperKind, HelperState};
use companion_wire::{Handover, RunsIn, TerminalSession, app_name_of};
use prov::{AgentProgram, HelperId, SessionId};
use std::collections::{BTreeMap, BTreeSet};

/// Where the helpers' conversations are. Not kept across a restart.
#[derive(Debug, Default)]
pub struct Terminals {
    /// The helpers whose agent app said, after its last job, that the person could pick its
    /// conversation up in the app's own terminal program.
    can_hand: BTreeSet<HelperId>,
    /// The helpers whose conversation is in the app's own terminal program: the session at the
    /// router that is a watch session meanwhile.
    away: BTreeMap<HelperId, SessionId>,
}

impl Terminals {
    /// Where the helper's conversation is running.
    pub fn runs_in(&self, helper: &HelperId) -> RunsIn {
        match self.away.contains_key(helper) {
            true => RunsIn::Terminal,
            false => RunsIn::Desktop,
        }
    }

    /// What the person can do about where the helper's conversation is: take it to the terminal
    /// when the app said it can be picked up and no job runs, or bring it back.
    pub fn handover_of(&self, card: &HelperCard, board: &Board) -> Handover {
        let idle = card.state == HelperState::Idle && board.running(&card.id).is_none();
        let ready = self.can_hand.contains(&card.id) && idle;
        match (self.away.contains_key(&card.id), ready) {
            (true, _) => Handover::BringBack,
            (false, true) => Handover::ToTerminal,
            (false, false) => Handover::Unavailable,
        }
    }

    /// The app's last job said its conversation can be picked up in the terminal.
    pub fn can_pick_up(&mut self, helper: &HelperId) {
        self.can_hand.insert(helper.clone());
    }

    /// The app's last job said it cannot.
    pub fn cannot_pick_up(&mut self, helper: &HelperId) {
        self.can_hand.remove(helper);
    }

    /// The conversation went to the terminal, as `session` at the router.
    pub fn went_away(&mut self, helper: &HelperId, session: SessionId) {
        self.away.insert(helper.clone(), session);
        self.can_hand.remove(helper);
    }

    /// The conversation came back from the terminal.
    pub fn came_back(&mut self, helper: &HelperId) {
        self.away.remove(helper);
        self.can_hand.insert(helper.clone());
    }

    /// The host no longer knows the conversation: it is not away any more, and nothing is offered.
    pub fn lost(&mut self, helper: &HelperId) {
        self.away.remove(helper);
    }

    /// The sessions of the conversations that are in a terminal.
    pub fn sessions_away(&self) -> Vec<SessionId> {
        self.away.values().cloned().collect()
    }
}

/// Why a helper's conversation cannot go to the terminal now.
pub fn why_not_to_terminal(card: &HelperCard, board: &Board) -> HandFault {
    let running = board.running(&card.id).is_some();
    match (&card.state, running) {
        (HelperState::Paused(_), _) => HandFault::Paused,
        (HelperState::Idle, false) => HandFault::NoConversation,
        _ => HandFault::Busy,
    }
}

/// The conversations a terminal started with an agent app that the person can bring in: the open
/// watch sessions that name their app, except the ones in `away` (a helper's own conversation).
pub fn terminal_sessions(
    watched: Vec<WatchedLine>,
    away: &[SessionId],
    handover: Handover,
) -> Vec<TerminalSession> {
    watched
        .into_iter()
        .filter(|line| !away.contains(&line.session))
        .filter_map(|line| {
            let program = AgentProgram::parse(line.agent.program.as_str()).ok()?;
            Some(TerminalSession {
                session: line.session,
                app_name: app_name_of(&HelperKind::AgentApp(program.clone())),
                program,
                folder: line.folder.map(|folder| folder.as_str().to_owned()),
                handover,
            })
        })
        .collect()
}
