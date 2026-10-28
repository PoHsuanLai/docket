//! A scripted host of agent apps: what it was asked and what it answers, by helper name, and the
//! outcomes the tests give it.

use crate::support::world::*;
use companion_wire::HelperReply;
use docket_core::helper::*;
use docket_core::{AppSessionId, SeenCommand, WatchedAgent};
use docket_permissions_core::{ActionMatch, ProgramName, TaskPolicy, TaskPolicyState};
use docket_tasks::{HelperDesk, PostFromPerson};
use docket_vocab::{LabelText, Workspace};
use porter_core::Count;
use prov::{AgentProgram, Effect, EntityKind, HelperId, SessionId, UnixSeconds};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub async fn until(what: &str, mut ready: impl FnMut() -> bool) {
    for _ in 0..500 {
        if ready() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never happened: {what}");
}

/// What a scripted host of agent apps was asked, and what it answers, by helper name.
#[derive(Default)]
pub struct Apps {
    pub runs: Mutex<BTreeMap<String, VecDeque<AgentOutcome>>>,
    pub sign_ins: Mutex<VecDeque<SignInOutcome>>,
    pub goals: Mutex<Vec<(String, String)>>,
    pub signed_with: Mutex<Vec<String>>,
    pub to_terminal: Mutex<VecDeque<ToTerminalOutcome>>,
    pub back: Mutex<VecDeque<BackOutcome>>,
    pub brought: Mutex<VecDeque<BroughtOutcome>>,
    pub brought_with: Mutex<Vec<(SessionId, String, Workspace)>>,
}

impl Apps {
    pub fn will_run(&self, helper: &str, outcomes: Vec<AgentOutcome>) {
        self.runs
            .lock()
            .expect("runs")
            .insert(helper.to_owned(), outcomes.into());
    }
}

impl AgentApps for Apps {
    fn run<'a>(&'a self, card: &'a HelperCard, job: &'a Job) -> AgentPending<'a, AgentOutcome> {
        Box::pin(async move {
            let name = card.name.as_str().to_owned();
            self.goals
                .lock()
                .expect("goals")
                .push((name.clone(), job.goal.as_str().to_owned()));
            self.runs
                .lock()
                .expect("runs")
                .get_mut(&name)
                .and_then(VecDeque::pop_front)
                .unwrap_or(AgentOutcome::Failed(JobFault::Crashed))
        })
    }

    fn stop<'a>(&'a self, _helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async {})
    }

    fn sign_in<'a>(
        &'a self,
        _card: &'a HelperCard,
        way: &'a SignInWay,
    ) -> AgentPending<'a, SignInOutcome> {
        Box::pin(async move {
            self.signed_with.lock().expect("ways").push(way.id.clone());
            self.sign_ins
                .lock()
                .expect("sign ins")
                .pop_front()
                .unwrap_or(SignInOutcome::Failed)
        })
    }

    fn check<'a>(&'a self, _card: &'a HelperCard) -> AgentPending<'a, CheckOutcome> {
        Box::pin(async { CheckOutcome::SignedIn })
    }

    fn sign_out<'a>(&'a self, _card: &'a HelperCard) -> AgentPending<'a, SignOutOutcome> {
        Box::pin(async { SignOutOutcome::SignedOut })
    }

    fn forget<'a>(&'a self, _helper: &'a HelperId) -> AgentPending<'a, ()> {
        Box::pin(async {})
    }

    fn to_terminal<'a>(&'a self, _card: &'a HelperCard) -> AgentPending<'a, ToTerminalOutcome> {
        Box::pin(async move {
            let next = self.to_terminal.lock().expect("to terminal").pop_front();
            next.unwrap_or(ToTerminalOutcome::Refused(HandFault::Failed))
        })
    }

    fn bring_back<'a>(&'a self, _card: &'a HelperCard) -> AgentPending<'a, BackOutcome> {
        Box::pin(async move {
            let next = self.back.lock().expect("back").pop_front();
            next.unwrap_or(BackOutcome::Refused(HandFault::Failed))
        })
    }

    /// Knows one app: a line that ends in its conversation `0a1b-2c3d`.
    fn recognise<'a>(&'a self, seen: &'a SeenCommand) -> AgentPending<'a, Option<WatchedAgent>> {
        Box::pin(async move {
            let id = seen
                .tokens()
                .last()
                .and_then(|t| AppSessionId::parse(t).ok())?;
            let program = ProgramName::parse("claude-code").ok()?;
            (id.as_str() == "0a1b-2c3d").then_some(WatchedAgent {
                program,
                session: id,
            })
        })
    }

    fn bring_back_watched<'a>(
        &'a self,
        session: &'a SessionId,
        agent: &'a WatchedAgent,
        folder: &'a Workspace,
    ) -> AgentPending<'a, BroughtOutcome> {
        Box::pin(async move {
            self.brought_with.lock().expect("brought with").push((
                session.clone(),
                agent.session.to_string(),
                folder.clone(),
            ));
            let next = self.brought.lock().expect("brought").pop_front();
            next.unwrap_or(BroughtOutcome::Refused(HandFault::Failed))
        })
    }
}

pub fn program() -> AgentProgram {
    AgentProgram::parse("claude-code").expect("program")
}

pub fn reach() -> TaskPolicy {
    TaskPolicy {
        task: task("t-0"),
        space: space("work"),
        from: vec![],
        actions: BTreeSet::from([ActionMatch::AppUpTo(
            app("org.quire.Mail"),
            Effect::UndoableWrite,
        )]),
        kinds: BTreeSet::from([EntityKind::parse("mail.thread").expect("kind")]),
        ceiling: Effect::UndoableWrite,
        max_count: Count(10),
        recipients: vec![],
        destinations: vec![],
        paths: vec![],
        expires: UnixSeconds(i64::MAX),
        rationale: LabelText::parse("what the helper may use").expect("words"),
        state: TaskPolicyState::Active,
    }
}

/// An app helper: no model is chosen for it, because the app brings its own.
pub fn app_draft(name: &str) -> CardDraft {
    CardDraft {
        name: HelperName::parse(name).expect("name"),
        role: Role::parse("Does the coding jobs.").expect("role"),
        kind: HelperKind::AgentApp(program()),
        space: space("work"),
        model: HelperModel::Unset,
        reach: reach(),
        budget: HelperBudget { steps: Count(8) },
    }
}

pub fn hub(w: &World, apps: &Arc<Apps>) -> TestHub {
    let hub = hub_of(w);
    hub.set_agents(apps.clone());
    hub
}

pub async fn make(hub: &TestHub, name: &str) -> HelperId {
    let effects = hub
        .apply(BoardEvent::Create(Box::new(app_draft(name))))
        .await;
    effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a card was made")
}

pub async fn give(hub: &TestHub, to: &HelperId, text: &str) -> JobId {
    let HelperReply::Posted(job) = hub
        .post_job(PostFromPerson {
            helper: to.clone(),
            goal: text.to_owned(),
            things: vec![],
        })
        .await
    else {
        panic!("a job is posted");
    };
    job
}

pub fn done(words: &str) -> AgentOutcome {
    AgentOutcome::Done {
        words: UntrustedWords::new(words.to_owned()),
        task: task("t-9"),
        session: SessionId::parse("s-9").expect("session"),
        own_session: None,
    }
}

/// A finished job whose app could pick its conversation up in its own terminal program.
pub fn done_resumable(words: &str) -> AgentOutcome {
    match done(words) {
        AgentOutcome::Done {
            words,
            task,
            session,
            ..
        } => AgentOutcome::Done {
            words,
            task,
            session,
            own_session: AppSessionId::parse("0a1b-2c3d").ok(),
        },
        other => other,
    }
}

pub fn line(program: &str, args: &[&str]) -> TerminalLine {
    TerminalLine {
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: BTreeMap::new(),
    }
}

/// The command that picks the conversation up, as the host resolves it, and the session that is
/// a watch session meanwhile.
pub fn terminal_hand() -> TerminalHand {
    TerminalHand {
        run: TerminalRun {
            plain: line("/usr/bin/claude", &["--resume", "0a1b-2c3d"]),
            folder: Some("/home/p/.local/share/docket/helpers/h-1".to_owned()),
            confined: Some(line(
                "/usr/bin/bwrap",
                &["--", "claude", "--resume", "0a1b-2c3d"],
            )),
        },
        session: SessionId::parse("s-9").expect("session"),
    }
}

pub fn ways() -> Vec<SignInWay> {
    vec![
        SignInWay {
            id: "claude-login".to_owned(),
            name: "Sign in with your Claude account".to_owned(),
            how: SignInHow::Agent,
        },
        SignInWay {
            id: "terminal-login".to_owned(),
            name: "Sign in in a terminal".to_owned(),
            how: SignInHow::Terminal(TerminalLogin {
                args: vec!["/login".to_owned()],
                env: BTreeMap::new(),
                run: None,
            }),
        },
    ]
}

pub fn status_of(hub: &TestHub, job: JobId) -> JobStatus {
    hub.board().job(job).expect("job").status.clone()
}

pub fn state_of(hub: &TestHub, helper: &HelperId) -> HelperState {
    hub.board().card(helper).expect("card").state.clone()
}
