//! A helper's conversation with its agent app moving between docket and the app's own
//! command-line program in a terminal ("Open in Terminal", "Bring Back"), and the conversations
//! a terminal started that the person can bring in. While a conversation is in the terminal its
//! helper is paused, so no job reaches the app; the router keeps the session as a watch session
//! the terminal marks, and bringing it back resumes the helper. Every refusal is a plain sentence.

use super::hub::Hub;
use companion_wire::{
    BroughtBack, Handover, HelperCardView, HelperLine, HelperReply, RunsIn, TerminalSession,
    app_name_of,
};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    BackOutcome, BoardEvent, BroughtOutcome, HandFault, HelperCard, HelperKind, HelperState,
    JobRefusal, TerminalLogin, TerminalRequest, ToTerminalOutcome,
};
use docket_core::{SeenCommand, WatchedAgent};
use porter_client::Transport as InferTransport;
use prov::{AgentProgram, HelperId, SessionId};

/// What a call that needs a host of apps says when there is none.
const NO_APPS: &str = "No apps can be reached here.";

impl<P, I> Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    /// Where the helper's conversation is running.
    fn runs_in(&self, helper: &HelperId) -> RunsIn {
        match self.with_state(|state| state.away.contains_key(helper)) {
            true => RunsIn::Terminal,
            false => RunsIn::Desktop,
        }
    }

    /// What the person can do about where the helper's conversation is: take it to the terminal
    /// when the app said it can be picked up and no job runs, or bring it back.
    fn handover_of(&self, card: &HelperCard) -> Handover {
        let board = self.board();
        let (away, ready) = self.with_state(|state| {
            (
                state.away.contains_key(&card.id),
                state.can_hand.contains(&card.id),
            )
        });
        let idle = card.state == HelperState::Idle && board.running(&card.id).is_none();
        match (away, ready && idle) {
            (true, _) => Handover::BringBack,
            (false, true) => Handover::ToTerminal,
            (false, false) => Handover::Unavailable,
        }
    }

    /// `line` as the person sees it: a conversation in the terminal says so.
    pub(super) fn line_shown(&self, mut line: HelperLine) -> HelperLine {
        if self.runs_in(&line.id) == RunsIn::Terminal {
            line.doing = "In its own terminal window".to_owned();
        }
        line
    }

    /// `view` with where the conversation runs and what can be done about it.
    pub(super) fn card_shown(&self, mut view: HelperCardView, card: &HelperCard) -> HelperCardView {
        view.runs_in = self.runs_in(&card.id);
        view.handover = self.handover_of(card);
        view.line = self.line_shown(view.line);
        view
    }

    /// The person opens the helper's conversation in the app's own program in a terminal.
    pub(super) async fn open_terminal_for(&self, helper: HelperId) -> HelperReply {
        let refused = |why: &str| HelperReply::Refused(why.to_owned());
        let Some(card) = self.board().card(&helper).cloned() else {
            return refused(&JobRefusal::NoSuchHelper.plain());
        };
        let Some(program) = card.kind.program().cloned() else {
            return refused("This helper does not run an app.");
        };
        let Some(agents) = self.agents() else {
            return refused(NO_APPS);
        };
        match self.handover_of(&card) {
            Handover::ToTerminal => {}
            Handover::BringBack => return refused("It is already in the terminal."),
            Handover::Unavailable => return refused(self.why_not_to_terminal(&card).plain()),
        }
        match agents.to_terminal(&card).await {
            ToTerminalOutcome::Ready(hand) => {
                self.with_state(|state| {
                    state.away.insert(helper.clone(), hand.session.clone());
                    state.can_hand.remove(&helper);
                });
                // No job reaches the app while the person works in it.
                self.apply(BoardEvent::Pause(helper.clone())).await;
                self.changed();
                let run = Box::new(hand.run);
                HelperReply::OpenTerminal(TerminalRequest {
                    helper,
                    program,
                    login: TerminalLogin {
                        run: Some(run.clone()),
                        ..TerminalLogin::default()
                    },
                    run: Some(run),
                    handoff: Some(hand.session),
                })
            }
            ToTerminalOutcome::Refused(why) => refused(why.plain()),
        }
    }

    /// Why a helper's conversation cannot go to the terminal now.
    fn why_not_to_terminal(&self, card: &HelperCard) -> HandFault {
        let running = self.board().running(&card.id).is_some();
        match (&card.state, running) {
            (HelperState::Paused(_), _) => HandFault::Paused,
            (HelperState::Idle, false) => HandFault::NoConversation,
            _ => HandFault::Busy,
        }
    }

    /// The person brings the helper's conversation back from the terminal.
    pub(super) async fn bring_back_for(&self, helper: HelperId) -> HelperReply {
        let refused = |why: &str| HelperReply::Refused(why.to_owned());
        let Some(card) = self.board().card(&helper).cloned() else {
            return refused(&JobRefusal::NoSuchHelper.plain());
        };
        let Some(agents) = self.agents() else {
            return refused(NO_APPS);
        };
        if self.runs_in(&helper) == RunsIn::Desktop {
            return refused(HandFault::NotThere.plain());
        }
        match agents.bring_back(&card).await {
            BackOutcome::Back => {
                self.with_state(|state| {
                    state.away.remove(&helper);
                    state.can_hand.insert(helper.clone());
                });
                self.apply(BoardEvent::Resume(helper)).await;
                self.changed();
                HelperReply::Done
            }
            // The host no longer knows the conversation (it was restarted): nothing is left to
            // bring back, so the helper goes on as it was.
            BackOutcome::Refused(HandFault::NotThere) => {
                self.with_state(|state| state.away.remove(&helper));
                self.apply(BoardEvent::Resume(helper)).await;
                self.changed();
                refused(HandFault::NotThere.plain())
            }
            BackOutcome::Refused(why) => refused(why.plain()),
        }
    }

    /// The conversations a terminal started with an agent app that the person can bring in: the
    /// open watch sessions that name their app, except the ones a helper's own conversation is.
    pub(super) async fn terminal_sessions_now(&self) -> Vec<TerminalSession> {
        let handover = match self.agents() {
            Some(_) => Handover::BringBack,
            None => Handover::Unavailable,
        };
        let watched = self.watched_sessions().await;
        let helpers = self.with_state(|state| state.away.values().cloned().collect::<Vec<_>>());
        watched
            .into_iter()
            .filter(|line| !helpers.contains(&line.session))
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

    /// The person brings a terminal's conversation into docket: the app starts on the same
    /// session and picks up its own conversation.
    pub(super) async fn bring_in(&self, session: SessionId) -> BroughtBack {
        let refused = |why: &str| BroughtBack::Refused(why.to_owned());
        let Some(agents) = self.agents() else {
            return refused(NO_APPS);
        };
        let watched = self.watched_sessions().await;
        let Some(line) = watched.into_iter().find(|line| line.session == session) else {
            return refused(HandFault::NotThere.plain());
        };
        let Some(folder) = line.folder else {
            return refused(HandFault::NotThere.plain());
        };
        match agents
            .bring_back_watched(&session, &line.agent, &folder)
            .await
        {
            BroughtOutcome::Brought(session) => {
                self.changed();
                BroughtBack::Session(session)
            }
            BroughtOutcome::Refused(why) => refused(why.plain()),
        }
    }

    /// The app and conversation a terminal's command line picks up, as the host of the apps
    /// reads its list. None when there is no host, or it knows no such app.
    pub(super) async fn recognise_seen(&self, seen: SeenCommand) -> Option<WatchedAgent> {
        self.agents()?.recognise(&seen).await
    }
}
