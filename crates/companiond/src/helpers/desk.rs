//! The hub as the desk the task model and the shell talk to: the planner's two helper actions,
//! and the shell's list, card and changes. Every refusal is a plain sentence.

use super::hub::Hub;
use companion_wire::{
    AnswerPhase, HelperCardView, HelperForm, HelperLine, HelperNeeds, HelperReply, HelpersView,
    LoginShown, SignInChoice, ToldWire, card_of, line_of,
};
use docket_client::Transport as IntentsTransport;
use docket_core::helper::{
    Board, BoardEffect, BoardEvent, CardDraft, GoalOrigin, GoalText, HelperBudget, HelperCard,
    HelperKind, HelperModel, HelperName, HelperState, JobRefusal, JobStatus, NewJob, PlannerSaw,
    ReachFault, Role, SignInOutcome, SignOutOutcome, TerminalRequest, reach_policy,
};
use docket_core::{HelperRosterLine, LeadText, Manifest};
use docket_tasks::{HelperDesk, Pending, PostFromPerson, PostFromPlanner, Proposal};
use porter_client::Transport as InferTransport;
use porter_core::{AppName, Count};
use prov::{HelperId, SpaceId};

/// How many steps a helper the assistant proposed may take in one job, until the person says.
const PROPOSED_STEPS: Count = Count(12);

fn reach_sentence(fault: &ReachFault) -> String {
    match fault {
        ReachFault::NoSuchApp(app) => format!("There is no app called {app}."),
        _ => "Choose at least one app for it to use.".to_owned(),
    }
}

fn taken(name: &HelperName) -> String {
    format!("There is already a helper called {}.", name.as_str())
}

fn job_reply(effects: &[BoardEffect]) -> HelperReply {
    for effect in effects {
        match effect {
            BoardEffect::JobRefused(why) => return HelperReply::Refused(why.plain()),
            BoardEffect::Posted(id) => return HelperReply::Posted(*id),
            _ => {}
        }
    }
    HelperReply::Refused("The job could not be given.".to_owned())
}

/// The latest job `name` finished.
fn latest_answer(board: &Board, name: &str) -> Option<docket_core::helper::JobId> {
    let card = board.by_name(name)?;
    board
        .jobs()
        .iter()
        .rev()
        .find(|j| j.owner == card.id && matches!(j.status, JobStatus::Done(_)))
        .map(|j| j.id)
}

impl<P, I> Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
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
    async fn draft_of(&self, form: HelperForm, space: &SpaceId) -> Result<CardDraft, String> {
        let manifests = self.manifests().await;
        let installed: Vec<&Manifest> = manifests.iter().map(|m| m.manifest()).collect();
        let reach = reach_policy(&form.apps, form.ceiling, space, &installed)
            .map_err(|fault| reach_sentence(&fault))?;
        Ok(CardDraft {
            name: form.name,
            role: form.role,
            kind: form.kind,
            space: space.clone(),
            model: form.model,
            reach,
            budget: HelperBudget {
                steps: Count(form.steps.0.max(1)),
            },
        })
    }

    /// The person chose a way of signing in to the app the helper runs.
    async fn sign_in_with(&self, helper: HelperId, way: String) -> HelperReply {
        let Some(card) = self.board().card(&helper).cloned() else {
            return HelperReply::Refused(JobRefusal::NoSuchHelper.plain());
        };
        let Some(program) = card.kind.program().cloned() else {
            return HelperReply::Refused(
                "This helper does not run an app to sign in to.".to_owned(),
            );
        };
        let Some(agents) = self.agents() else {
            return HelperReply::Refused("No apps can be signed in to here.".to_owned());
        };
        let listed = self.with_state(|state| state.signins.get(&helper).cloned());
        let Some(way) = listed.and_then(|ways| ways.into_iter().find(|w| w.id == way)) else {
            return HelperReply::Refused("That is not one of the ways to sign in.".to_owned());
        };
        match agents.sign_in(&card, &way).await {
            SignInOutcome::SignedIn => {
                self.signed_in_to_app_of(&helper).await;
                HelperReply::Done
            }
            SignInOutcome::OpenTerminal(login) => HelperReply::OpenTerminal(TerminalRequest {
                helper,
                program,
                run: login.run.clone(),
                login,
            }),
            SignInOutcome::Refused => HelperReply::Refused(
                "It did not sign you in. Try again, or choose another way.".to_owned(),
            ),
            SignInOutcome::Failed => HelperReply::Refused("It could not be started.".to_owned()),
        }
    }

    /// The person signed out of the app the helper runs; every helper that runs it is signed out.
    async fn sign_out_of(&self, helper: HelperId) -> HelperReply {
        let Some(card) = self.board().card(&helper).cloned() else {
            return HelperReply::Refused(JobRefusal::NoSuchHelper.plain());
        };
        let (Some(program), Some(agents)) = (card.kind.program().cloned(), self.agents()) else {
            return HelperReply::Refused(
                "This helper does not run an app to sign out of.".to_owned(),
            );
        };
        match agents.sign_out(&card).await {
            SignOutOutcome::SignedOut => {
                self.with_state(|state| {
                    let same: Vec<HelperId> = state
                        .board
                        .cards()
                        .iter()
                        .filter(|c| c.kind.program() == Some(&program))
                        .map(|c| c.id.clone())
                        .collect();
                    for id in same {
                        state.signed.remove(&id);
                    }
                });
                HelperReply::Done
            }
            SignOutOutcome::Failed => HelperReply::Refused("Signing out did not work.".to_owned()),
        }
    }

    fn name_taken(&self, name: &HelperName, except: Option<&HelperId>) -> bool {
        self.board()
            .by_name(name.as_str())
            .is_some_and(|card| Some(&card.id) != except)
    }

    async fn make(&self, form: HelperForm) -> HelperReply {
        if self.name_taken(&form.name, None) {
            return HelperReply::Refused(taken(&form.name));
        }
        let space = form.space.clone();
        let draft = match self.draft_of(form, &space).await {
            Ok(draft) => draft,
            Err(why) => return HelperReply::Refused(why),
        };
        let effects = self.apply(BoardEvent::Create(Box::new(draft))).await;
        effects
            .into_iter()
            .find_map(|effect| match effect {
                BoardEffect::Created(id) => Some(HelperReply::Made(id)),
                _ => None,
            })
            .unwrap_or_else(|| HelperReply::Refused("The helper could not be made.".to_owned()))
    }

    async fn change(&self, helper: HelperId, form: HelperForm) -> HelperReply {
        let Some(space) = self.board().card(&helper).map(|c| c.space.clone()) else {
            return HelperReply::Refused(JobRefusal::NoSuchHelper.plain());
        };
        if self.name_taken(&form.name, Some(&helper)) {
            return HelperReply::Refused(taken(&form.name));
        }
        match self.draft_of(form, &space).await {
            Ok(draft) => {
                self.apply(BoardEvent::Edit {
                    helper,
                    draft: Box::new(draft),
                })
                .await;
                HelperReply::Done
            }
            Err(why) => HelperReply::Refused(why),
        }
    }

    async fn give(&self, ask: PostFromPerson) -> HelperReply {
        if self.board().card(&ask.helper).is_none() {
            return HelperReply::Refused(JobRefusal::NoSuchHelper.plain());
        }
        let Ok(goal) = GoalText::parse(&ask.goal) else {
            return HelperReply::Refused("Say what it is to do, in a few sentences.".to_owned());
        };
        let effects = self
            .apply(BoardEvent::Post(Box::new(NewJob {
                to: ask.helper,
                goal,
                origin: GoalOrigin::Person,
                giver: None,
                owns: ask.things,
                with: Vec::new(),
            })))
            .await;
        job_reply(&effects)
    }

    async fn give_for_planner(&self, ask: PostFromPlanner) -> HelperReply {
        let board = self.board();
        // A helper in another Space is not reachable from this one: Spaces are walls.
        let Some(card) = board
            .by_name(&ask.helper)
            .filter(|card| card.space == ask.space)
        else {
            return HelperReply::Refused(JobRefusal::NoSuchHelper.plain());
        };
        let Ok(goal) = GoalText::parse(&ask.goal) else {
            return HelperReply::Refused("Say what it is to do, in a few sentences.".to_owned());
        };
        let with = match &ask.after {
            None => Vec::new(),
            Some(name) => match latest_answer(&board, name) {
                Some(job) => vec![job],
                None => {
                    return HelperReply::Refused(format!(
                        "{name} has nothing finished to hand on."
                    ));
                }
            },
        };
        // The router refused this unless the planner had read nothing untrusted; the board asks
        // again, and says what it was told.
        let effects = self
            .apply(BoardEvent::Post(Box::new(NewJob {
                to: card.id.clone(),
                goal,
                origin: GoalOrigin::Planner(PlannerSaw::Nothing),
                giver: Some(ask.giver),
                owns: Vec::new(),
                with,
            })))
            .await;
        job_reply(&effects)
    }

    async fn proposed(&self, proposal: Proposal) -> HelperReply {
        let (Ok(name), Ok(role)) = (
            HelperName::parse(&proposal.name),
            Role::parse(&proposal.role),
        ) else {
            return HelperReply::Refused(
                "The name or what it is for is empty or too long.".to_owned(),
            );
        };
        let apps: Result<Vec<AppName>, String> = proposal
            .apps
            .iter()
            .map(|app| AppName::parse(app).map_err(|_| format!("There is no app called {app}.")))
            .collect();
        match apps {
            Ok(apps) => {
                self.make(HelperForm {
                    name,
                    role,
                    space: proposal.space,
                    model: HelperModel::Unset,
                    apps,
                    ceiling: proposal.ceiling,
                    steps: PROPOSED_STEPS,
                    kind: HelperKind::Native,
                })
                .await
            }
            Err(why) => HelperReply::Refused(why),
        }
    }
}

impl<P, I> HelperDesk for Hub<P, I>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    fn view(&self, after: u64) -> HelpersView {
        let board = self.board();
        HelpersView {
            helpers: board
                .cards()
                .iter()
                .map(|card| HelperLine {
                    conversation: self.conversation_of(&card.id),
                    ..line_of(card, &board, self.needs_of(card))
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
        let (signed, ways) = self.with_state(|state| {
            (
                state.signed.contains(helper),
                state.signins.get(helper).cloned().unwrap_or_default(),
            )
        });
        if signed && view.login == LoginShown::Unknown {
            view.login = LoginShown::SignedIn;
        }
        view.sign_in = ways.iter().map(SignInChoice::from).collect();
        Some(view)
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
            let paused = matches!(
                self.board().card(&helper).map(|c| &c.state),
                Some(HelperState::Paused(_))
            );
            let event = match paused {
                true => BoardEvent::Resume(helper),
                false => BoardEvent::Retry(helper),
            };
            self.apply(event).await;
        })
    }

    fn remove(&self, helper: HelperId) -> Pending<'_, ()> {
        Box::pin(async move {
            self.apply(BoardEvent::Remove(helper.clone())).await;
            self.with_state(|state| {
                state.signed.remove(&helper);
                state.signins.remove(&helper);
            });
            // The helper's agent process ends with it.
            if let Some(agents) = self.agents() {
                agents.forget(&helper).await;
            }
        })
    }

    fn post_job(&self, ask: PostFromPerson) -> Pending<'_, HelperReply> {
        Box::pin(self.give(ask))
    }

    fn post_from_planner(&self, ask: PostFromPlanner) -> Pending<'_, HelperReply> {
        Box::pin(self.give_for_planner(ask))
    }

    fn propose(&self, proposal: Proposal) -> Pending<'_, HelperReply> {
        Box::pin(self.proposed(proposal))
    }

    fn sign_in(&self, helper: HelperId, way: String) -> Pending<'_, HelperReply> {
        Box::pin(self.sign_in_with(helper, way))
    }

    fn sign_out(&self, helper: HelperId) -> Pending<'_, HelperReply> {
        Box::pin(self.sign_out_of(helper))
    }

    fn roster_lines(&self, except: Option<&HelperId>) -> Vec<HelperRosterLine> {
        let board = self.board();
        board
            .cards()
            .iter()
            .filter(|card| Some(&card.id) != except)
            .map(|card| HelperRosterLine {
                name: card.name.clone(),
                space: card.space.clone(),
                state: card.state.phase(),
                doing: Some(LeadText::of(&line_of(card, &board, None).doing)),
            })
            .collect()
    }
}
