//! A helper's conversation with its agent app moving between docket and the app's own
//! command-line program in a terminal ("Open in Terminal", "Bring Back"), and the conversations
//! a terminal started that the person can bring in. While a conversation is in the terminal its
//! helper is paused, so no job reaches the app; the router keeps the session as a watch session
//! the terminal marks, and bringing it back resumes the helper. The rules are
//! `companion_core::Terminals`'; every refusal is a typed `Refusal`, told in plain words at the
//! edge.

use super::owner::Hub;
use crate::words::{refused, sentence};
use companion_core::{AppStep, Refusal, terminal_sessions, why_not_to_terminal};
use companion_model::helper::{
    BackOutcome, BoardEvent, BroughtOutcome, HandFault, HelperCard, JobRefusal, TerminalLogin,
    TerminalRequest, ToTerminalOutcome,
};
use companion_model::{SeenCommand, WatchedAgent};
use companion_wire::{
    BroughtBack, Handover, HelperCardView, HelperLine, HelperReply, RunsIn, TerminalSession,
};
use docket_client::Transport as IntentsTransport;
use docket_planner::Planner;
use docket_tasks::Surface;
use docket_vocab::Clock;
use prov::{HelperId, SessionId};

impl<P, I, K, S> Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// `line` as the person sees it: a conversation in the terminal says so.
    pub(super) fn line_shown(&self, mut line: HelperLine) -> HelperLine {
        if self.with_state(|state| state.terminals.runs_in(&line.id)) == RunsIn::Terminal {
            line.doing = "In its own terminal window".to_owned();
        }
        line
    }

    /// `view` with where the conversation runs and what can be done about it.
    pub(super) fn card_shown(&self, mut view: HelperCardView, card: &HelperCard) -> HelperCardView {
        let (runs_in, handover) = self.with_state(|state| {
            (
                state.terminals.runs_in(&card.id),
                state.terminals.handover_of(card, &state.board),
            )
        });
        view.runs_in = runs_in;
        view.handover = handover;
        view.line = self.line_shown(view.line);
        view
    }

    /// The person opens the helper's conversation in the app's own program in a terminal.
    pub(super) async fn open_terminal_for(&self, helper: HelperId) -> HelperReply {
        match self.try_open_terminal(helper).await {
            Ok(reply) => reply,
            Err(why) => refused(why),
        }
    }

    async fn try_open_terminal(&self, helper: HelperId) -> Result<HelperReply, Refusal> {
        let card = self
            .board()
            .card(&helper)
            .cloned()
            .ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
        let program = card
            .kind
            .program()
            .cloned()
            .ok_or(Refusal::NotAnApp(AppStep::Terminal))?;
        let agents = self.agents().ok_or(Refusal::NoAppHost(AppStep::Terminal))?;
        let handover = self.with_state(|state| state.terminals.handover_of(&card, &state.board));
        match handover {
            Handover::ToTerminal => {}
            Handover::BringBack => return Err(Refusal::AlreadyInTerminal),
            Handover::Unavailable => {
                return Err(Refusal::Hand(why_not_to_terminal(&card, &self.board())));
            }
        }
        match agents.to_terminal(&card).await {
            ToTerminalOutcome::Ready(hand) => {
                self.with_state(|state| {
                    state.terminals.went_away(&helper, hand.session.clone());
                });
                // No job reaches the app while the person works in it.
                self.apply(BoardEvent::Pause(helper.clone())).await;
                self.changed();
                let run = Box::new(hand.run);
                Ok(HelperReply::OpenTerminal(TerminalRequest {
                    helper,
                    program,
                    login: TerminalLogin {
                        run: Some(run.clone()),
                        ..TerminalLogin::default()
                    },
                    run: Some(run),
                    handoff: Some(hand.session),
                }))
            }
            ToTerminalOutcome::Refused(why) => Err(Refusal::Hand(why)),
        }
    }

    /// The person brings the helper's conversation back from the terminal.
    pub(super) async fn bring_back_for(&self, helper: HelperId) -> HelperReply {
        match self.try_bring_back(helper).await {
            Ok(reply) => reply,
            Err(why) => refused(why),
        }
    }

    async fn try_bring_back(&self, helper: HelperId) -> Result<HelperReply, Refusal> {
        let card = self
            .board()
            .card(&helper)
            .cloned()
            .ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
        let agents = self.agents().ok_or(Refusal::NoAppHost(AppStep::Terminal))?;
        if self.with_state(|state| state.terminals.runs_in(&helper)) == RunsIn::Desktop {
            return Err(Refusal::Hand(HandFault::NotThere));
        }
        match agents.bring_back(&card).await {
            BackOutcome::Back => {
                self.with_state(|state| state.terminals.came_back(&helper));
                self.apply(BoardEvent::Resume(helper)).await;
                self.changed();
                Ok(HelperReply::Done)
            }
            // The host no longer knows the conversation (it was restarted): nothing is left to
            // bring back, so the helper goes on as it was.
            BackOutcome::Refused(HandFault::NotThere) => {
                self.with_state(|state| state.terminals.lost(&helper));
                self.apply(BoardEvent::Resume(helper)).await;
                self.changed();
                Err(Refusal::Hand(HandFault::NotThere))
            }
            BackOutcome::Refused(why) => Err(Refusal::Hand(why)),
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
        let away = self.with_state(|state| state.terminals.sessions_away());
        terminal_sessions(watched, &away, handover)
    }

    /// The person brings a terminal's conversation into docket: the app starts on the same
    /// session and picks up its own conversation.
    pub(super) async fn bring_in(&self, session: SessionId) -> BroughtBack {
        match self.try_bring_in(session).await {
            Ok(session) => BroughtBack::Session(session),
            Err(why) => BroughtBack::Refused(sentence(&why)),
        }
    }

    async fn try_bring_in(&self, session: SessionId) -> Result<SessionId, Refusal> {
        let agents = self.agents().ok_or(Refusal::NoAppHost(AppStep::Terminal))?;
        let watched = self.watched_sessions().await;
        let line = watched
            .into_iter()
            .find(|line| line.session == session)
            .ok_or(Refusal::Hand(HandFault::NotThere))?;
        let folder = line.folder.ok_or(Refusal::Hand(HandFault::NotThere))?;
        match agents
            .bring_back_watched(&session, &line.agent, &folder)
            .await
        {
            BroughtOutcome::Brought(session) => {
                self.changed();
                Ok(session)
            }
            BroughtOutcome::Refused(why) => Err(Refusal::Hand(why)),
        }
    }

    /// The app and conversation a terminal's command line picks up, as the host of the apps
    /// reads its list. None when there is no host, or it knows no such app.
    pub(super) async fn recognise_seen(&self, seen: SeenCommand) -> Option<WatchedAgent> {
        self.agents()?.recognise(&seen).await
    }
}
