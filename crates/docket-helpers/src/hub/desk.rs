//! The hub as the desk the task model and the shell talk to: the planner's two helper actions,
//! and the shell's list, card and changes. The decisions are `companion_core`'s and refuse with
//! a typed `Refusal`; the plain sentence is made here, at the edge (`crate::words`).

use super::owner::Hub;
use crate::words::{refused, sentence};
use companion_core::{
    PlannerJob, Refusal, RoutineWho, RoutineWish, card_draft, created, helper_form, name_free,
    person_job, planner_job, posted, resume_event, roster_lines, routine_wish, same_app,
    say_outcome, talkable,
};
use companion_wire::{
    AnswerPhase, BroughtBack, HelperCardView, HelperForm, HelperLine, HelperNeeds, HelperReply,
    HelpersView, LoginShown, ProposalId, RoutineProposal, SayReply, SignInChoice, TerminalSession,
    ToldWire, card_of, line_of,
};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    BoardEvent, CardDraft, HelperCard, JobId, JobRefusal, SignInOutcome, SignOutOutcome,
    TerminalRequest,
};
use docket_core::{HelperRosterLine, Manifest, SeenCommand, WatchedAgent};
use docket_planner::Planner;
use docket_tasks::{
    HelperDesk, Pending, PostFromPerson, PostFromPlanner, Proposal, RoutineAsk, RoutineFor,
    SayFromPerson, Surface,
};
use docket_vocab::Clock;
use prov::{HelperId, SessionId, SpaceId};

impl<P, I, K, S> Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    /// Whether the job a helper is working on waits for the person: a question, or a sheet.
    fn needs_of(&self, card: &HelperCard) -> Option<HelperNeeds> {
        let at = card.working_on()?;
        self.with_state(|state| {
            let answer = state
                .running
                .get(&at.job)?
                .shared
                .as_ref()?
                .answer(&at.task)?;
            matches!(answer.phase, AnswerPhase::NeedsYou(_)).then_some(HelperNeeds::Answer)
        })
    }

    /// The card the person confirmed, from the form: the reach is built from the apps named, over
    /// the apps installed.
    async fn draft_of(&self, form: HelperForm, space: &SpaceId) -> Result<CardDraft, Refusal> {
        let manifests = self.manifests().await;
        let installed: Vec<&Manifest> = manifests.iter().map(|m| m.manifest()).collect();
        card_draft(form, space, &installed)
    }

    /// The person chose a way of signing in to the app the helper runs.
    async fn sign_in_with(&self, helper: HelperId, way: String) -> HelperReply {
        match self.try_sign_in(helper, &way).await {
            Ok(reply) => reply,
            Err(why) => refused(why),
        }
    }

    async fn try_sign_in(&self, helper: HelperId, way: &str) -> Result<HelperReply, Refusal> {
        let card = self
            .board()
            .card(&helper)
            .cloned()
            .ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
        let program = card
            .kind
            .program()
            .cloned()
            .ok_or(Refusal::NotAnAppToSignInTo)?;
        let agents = self.agents().ok_or(Refusal::NoAppHostToSignInTo)?;
        let way = self
            .with_state(|state| state.logins.way(&helper, way))
            .ok_or(Refusal::NotAWayToSignIn)?;
        match agents.sign_in(&card, &way).await {
            SignInOutcome::SignedIn => {
                self.signed_in_to_app_of(&helper).await;
                Ok(HelperReply::Done)
            }
            SignInOutcome::OpenTerminal(login) => Ok(HelperReply::OpenTerminal(TerminalRequest {
                helper,
                program,
                run: login.run.clone(),
                login,
                handoff: None,
            })),
            SignInOutcome::Refused => Err(Refusal::SignInDeclined),
            SignInOutcome::Failed => Err(Refusal::SignInNotStarted),
        }
    }

    /// The person signed out of the app the helper runs; every helper that runs it is signed out.
    async fn sign_out_of(&self, helper: HelperId) -> HelperReply {
        let Some(card) = self.board().card(&helper).cloned() else {
            return refused(Refusal::Job(JobRefusal::NoSuchHelper));
        };
        let (Some(program), Some(agents)) = (card.kind.program().cloned(), self.agents()) else {
            return refused(Refusal::NotAnAppToSignOutOf);
        };
        match agents.sign_out(&card).await {
            SignOutOutcome::SignedOut => {
                self.with_state(|state| {
                    let same = same_app(&state.board, &program);
                    state.logins.signed_out(same.all);
                });
                HelperReply::Done
            }
            SignOutOutcome::Failed => refused(Refusal::SignOutFailed),
        }
    }

    async fn make(&self, form: HelperForm) -> HelperReply {
        match self.try_make(form).await {
            Ok(id) => HelperReply::Made(id),
            Err(why) => refused(why),
        }
    }

    async fn try_make(&self, form: HelperForm) -> Result<HelperId, Refusal> {
        name_free(&self.board(), &form.name, None)?;
        let space = form.space.clone();
        let draft = self.draft_of(form, &space).await?;
        let effects = self.apply(BoardEvent::Create(Box::new(draft))).await;
        created(&effects)
    }

    async fn change(&self, helper: HelperId, form: HelperForm) -> HelperReply {
        match self.try_change(helper, form).await {
            Ok(()) => HelperReply::Done,
            Err(why) => refused(why),
        }
    }

    async fn try_change(&self, helper: HelperId, form: HelperForm) -> Result<(), Refusal> {
        let space = self
            .board()
            .card(&helper)
            .map(|c| c.space.clone())
            .ok_or(Refusal::Job(JobRefusal::NoSuchHelper))?;
        name_free(&self.board(), &form.name, Some(&helper))?;
        let draft = self.draft_of(form, &space).await?;
        self.apply(BoardEvent::Edit {
            helper,
            draft: Box::new(draft),
        })
        .await;
        Ok(())
    }

    async fn give(&self, ask: PostFromPerson) -> HelperReply {
        match self.try_give(ask).await {
            Ok(id) => HelperReply::Posted(id),
            Err(why) => refused(why),
        }
    }

    async fn try_give(&self, ask: PostFromPerson) -> Result<JobId, Refusal> {
        let job = person_job(&self.board(), &ask.helper, &ask.goal, ask.things)?;
        posted(&self.apply(BoardEvent::Post(Box::new(job))).await)
    }

    /// The person speaks in a helper's conversation. Only a helper's conversation is spoken in: a
    /// session that is not one is refused. The turn runs on the companion kept for that helper,
    /// in its voice, so its model, pool and reach are the card's and its calls go through the
    /// router's gate under its seat.
    async fn speak(&self, say: SayFromPerson) -> SayReply {
        match self.try_speak(say).await {
            Ok(words) => SayReply::Said(words),
            Err(why) => SayReply::Refused(sentence(&why)),
        }
    }

    async fn try_speak(&self, say: SayFromPerson) -> Result<String, Refusal> {
        let (card, talk) = self
            .conversation_at(&say.session)
            .ok_or(Refusal::NotAConversation)?;
        talkable(&card)?;
        let companion = self.talker(&card);
        let report = companion.lock().await.talk(&card, &talk, say).await;
        say_outcome(&card, report)
    }

    async fn give_for_planner(&self, ask: PostFromPlanner) -> HelperReply {
        match self.try_give_for_planner(ask).await {
            Ok(id) => HelperReply::Posted(id),
            Err(why) => refused(why),
        }
    }

    async fn try_give_for_planner(&self, ask: PostFromPlanner) -> Result<JobId, Refusal> {
        let job = planner_job(
            &self.board(),
            PlannerJob {
                helper: &ask.helper,
                space: &ask.space,
                goal: &ask.goal,
                after: ask.after.as_deref(),
                giver: &ask.giver,
                origin: ask.origin,
            },
        )?;
        posted(&self.apply(BoardEvent::Post(Box::new(job))).await)
    }

    /// Keeps a proposed routine for the person to accept. Nothing is made: the goal, the time and
    /// the why are a model's words, shown to the person as data.
    fn routine_proposed(&self, ask: RoutineAsk) -> HelperReply {
        let who = match &ask.helper {
            RoutineFor::Named(name) => RoutineWho::Named(name, &ask.space),
            RoutineFor::Itself(id) => RoutineWho::Itself(id),
        };
        let wish = RoutineWish {
            who,
            goal: &ask.goal,
            when: &ask.when,
            why: &ask.why,
        };
        let (form, why) = match routine_wish(&self.board(), wish) {
            Ok(proposal) => proposal,
            Err(why) => return refused(why),
        };
        match self.with_state(|state| state.proposals.add(form, why)) {
            Some(_) => {
                self.changed();
                HelperReply::Done
            }
            None => refused(Refusal::TooManyWaiting),
        }
    }

    async fn proposed(&self, proposal: Proposal) -> HelperReply {
        let form = helper_form(
            &proposal.name,
            &proposal.role,
            &proposal.apps,
            proposal.ceiling,
            proposal.space,
        );
        match form {
            Ok(form) => self.make(form).await,
            Err(why) => refused(why),
        }
    }
}

impl<P, I, K, S> HelperDesk for Hub<P, I, K, S>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
    S: Surface + Send + Sync + 'static,
{
    fn view(&self, after: u64) -> HelpersView {
        let board = self.board();
        HelpersView {
            helpers: board
                .cards()
                .iter()
                .map(|card| {
                    self.line_shown(HelperLine {
                        conversation: self.conversation_of(&card.id),
                        ..line_of(card, &board, self.needs_of(card))
                    })
                })
                .collect::<Vec<HelperLine>>(),
            told: self
                .told_since(after)
                .into_iter()
                .map(|t| ToldWire {
                    n: t.n,
                    helper: t.helper,
                    text: t.text,
                })
                .collect(),
        }
    }

    fn card(&self, helper: &HelperId) -> Option<HelperCardView> {
        let board = self.board();
        let card = board.card(helper)?;
        let mut view = card_of(card, &board, self.needs_of(card), |job| self.job_words(job));
        let (signed, ways) =
            self.with_state(|state| (state.logins.is_signed(helper), state.logins.ways_of(helper)));
        if signed && view.login == LoginShown::Unknown {
            view.login = LoginShown::SignedIn;
        }
        view.sign_in = ways.iter().map(SignInChoice::from).collect();
        Some(self.card_shown(view, card))
    }

    fn create(&self, form: HelperForm) -> Pending<'_, HelperReply> {
        Box::pin(self.make(form))
    }

    fn edit(&self, helper: HelperId, form: HelperForm) -> Pending<'_, HelperReply> {
        Box::pin(self.change(helper, form))
    }

    fn pause(&self, helper: HelperId) -> Pending<'_, ()> {
        Box::pin(async move {
            self.apply(BoardEvent::Pause(helper)).await;
        })
    }

    fn resume(&self, helper: HelperId) -> Pending<'_, ()> {
        Box::pin(async move {
            let event = resume_event(&self.board(), helper);
            self.apply(event).await;
        })
    }

    fn remove(&self, helper: HelperId) -> Pending<'_, ()> {
        Box::pin(async move {
            self.apply(BoardEvent::Remove(helper.clone())).await;
            self.with_state(|state| state.logins.forget(&helper));
            // The helper's agent process ends with it.
            if let Some(agents) = self.agents() {
                agents.forget(&helper).await;
            }
        })
    }

    fn post_job(&self, ask: PostFromPerson) -> Pending<'_, HelperReply> {
        Box::pin(self.give(ask))
    }

    fn say(&self, say: SayFromPerson) -> Pending<'_, SayReply> {
        Box::pin(self.speak(say))
    }

    fn post_from_planner(&self, ask: PostFromPlanner) -> Pending<'_, HelperReply> {
        Box::pin(self.give_for_planner(ask))
    }

    fn propose(&self, proposal: Proposal) -> Pending<'_, HelperReply> {
        Box::pin(self.proposed(proposal))
    }

    fn propose_routine(&self, ask: RoutineAsk) -> Pending<'_, HelperReply> {
        Box::pin(async move { self.routine_proposed(ask) })
    }

    fn routine_proposals(&self) -> Vec<RoutineProposal> {
        self.with_state(|state| state.proposals.list())
    }

    fn take_routine_proposal(&self, id: ProposalId) -> Option<RoutineProposal> {
        let taken = self.with_state(|state| state.proposals.take(id));
        if taken.is_some() {
            self.changed();
        }
        taken
    }

    fn sign_in(&self, helper: HelperId, way: String) -> Pending<'_, HelperReply> {
        Box::pin(self.sign_in_with(helper, way))
    }

    fn sign_out(&self, helper: HelperId) -> Pending<'_, HelperReply> {
        Box::pin(self.sign_out_of(helper))
    }

    fn open_in_terminal(&self, helper: HelperId) -> Pending<'_, HelperReply> {
        Box::pin(self.open_terminal_for(helper))
    }

    fn bring_back(&self, helper: HelperId) -> Pending<'_, HelperReply> {
        Box::pin(self.bring_back_for(helper))
    }

    fn terminal_sessions(&self) -> Pending<'_, Vec<TerminalSession>> {
        Box::pin(self.terminal_sessions_now())
    }

    fn bring_back_terminal(&self, session: SessionId) -> Pending<'_, BroughtBack> {
        Box::pin(self.bring_in(session))
    }

    fn recognise(&self, seen: SeenCommand) -> Pending<'_, Option<WatchedAgent>> {
        Box::pin(self.recognise_seen(seen))
    }

    fn roster_lines(&self, except: Option<&HelperId>) -> Vec<HelperRosterLine> {
        roster_lines(&self.board(), except)
    }
}
