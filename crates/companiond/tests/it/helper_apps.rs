//! Helpers that run an agent app (swarm H1), against a scripted host of agent apps: a board job
//! is the app's one turn, its words come back as the job's untrusted answer, an app that wants
//! signing in makes its helper wait and never fail a job, the app's own sign-in ways are the
//! card's choices, and a crash ends that job only. No process runs and no login exists.

use crate::support::world::*;
use companion_wire::{
    BroughtBack, Handover, HelperNeeds, HelperReply, JobShown, LoginShown, RunsIn,
};
use docket_client::{InProcess, Intents};
use docket_core::helper::*;
use docket_core::{
    ActionMatch, CallerRole, LabelText, ProgramName, Rewind, TaskPolicy, TaskPolicyState,
    WatchedAgent, Workspace,
};
use docket_tasks::{HelperDesk, PostFromPerson};
use porter_core::Count;
use prov::{AgentProgram, Effect, EntityKind, HelperId, SessionId, UnixSeconds};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Duration;

async fn until(what: &str, mut ready: impl FnMut() -> bool) {
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
struct Apps {
    runs: Mutex<BTreeMap<String, VecDeque<AgentOutcome>>>,
    sign_ins: Mutex<VecDeque<SignInOutcome>>,
    goals: Mutex<Vec<(String, String)>>,
    signed_with: Mutex<Vec<String>>,
    to_terminal: Mutex<VecDeque<ToTerminalOutcome>>,
    back: Mutex<VecDeque<BackOutcome>>,
    brought: Mutex<VecDeque<BroughtOutcome>>,
    brought_with: Mutex<Vec<(SessionId, String, Workspace)>>,
}

impl Apps {
    fn will_run(&self, helper: &str, outcomes: Vec<AgentOutcome>) {
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

    fn bring_back_watched<'a>(
        &'a self,
        session: &'a SessionId,
        agent: &'a WatchedAgent,
        folder: &'a Workspace,
    ) -> AgentPending<'a, BroughtOutcome> {
        Box::pin(async move {
            self.brought_with.lock().expect("brought with").push((
                session.clone(),
                agent.session.clone(),
                folder.clone(),
            ));
            let next = self.brought.lock().expect("brought").pop_front();
            next.unwrap_or(BroughtOutcome::Refused(HandFault::Failed))
        })
    }
}

fn program() -> AgentProgram {
    AgentProgram::parse("claude-code").expect("program")
}

fn reach() -> TaskPolicy {
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
fn app_draft(name: &str) -> CardDraft {
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

fn hub(w: &World, apps: &Arc<Apps>) -> TestHub {
    let hub = hub_of(w);
    hub.set_agents(apps.clone());
    hub
}

async fn make(hub: &TestHub, name: &str) -> HelperId {
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

async fn give(hub: &TestHub, to: &HelperId, text: &str) -> JobId {
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

fn done(words: &str) -> AgentOutcome {
    AgentOutcome::Done {
        words: UntrustedWords::new(words.to_owned()),
        task: task("t-9"),
        session: SessionId::parse("s-9").expect("session"),
        own_session: None,
    }
}

/// A finished job whose app could pick its conversation up in its own terminal program.
fn done_resumable(words: &str) -> AgentOutcome {
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
            own_session: Some("0a1b-2c3d".to_owned()),
        },
        other => other,
    }
}

fn line(program: &str, args: &[&str]) -> TerminalLine {
    TerminalLine {
        program: program.to_owned(),
        args: args.iter().map(|a| (*a).to_owned()).collect(),
        env: BTreeMap::new(),
    }
}

/// The command that picks the conversation up, as the host resolves it, and the session that is
/// a watch session meanwhile.
fn terminal_hand() -> TerminalHand {
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

fn ways() -> Vec<SignInWay> {
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

fn status_of(hub: &TestHub, job: JobId) -> JobStatus {
    hub.board().job(job).expect("job").status.clone()
}

fn state_of(hub: &TestHub, helper: &HelperId) -> HelperState {
    hub.board().card(helper).expect("card").state.clone()
}

#[tokio::test]
async fn an_app_helper_takes_a_board_job_and_its_words_are_the_answer() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![done("Three offers, the second is best.")]);
    let hub = hub(&w, &apps);
    // No model is chosen and none is needed: the app brings its own.
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;

    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
    assert_eq!(
        hub.answer_words(job).as_deref(),
        Some("Three offers, the second is best.")
    );
    // The goal is the whole of what the app was told; no model of ours was asked.
    assert_eq!(
        apps.goals.lock().expect("goals").clone(),
        vec![("Coder".to_owned(), "compare the three offers".to_owned())]
    );
    assert!(w.infer.asked().is_empty());
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);
    assert_eq!(
        hub.card(&coder).expect("card").kind,
        HelperKind::AgentApp(program())
    );
}

#[tokio::test]
async fn an_app_that_wants_signing_in_makes_its_helper_wait_and_fails_no_job() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![AgentOutcome::NeedsSignIn(ways())]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;

    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;
    assert_eq!(status_of(&hub, job), JobStatus::Blocked(Need::SignIn));
    assert_eq!(
        hub.board().job(job).expect("job").failed,
        None,
        "not a failed job"
    );

    // The list says what it needs; the person is told once, in plain words.
    let view = hub.view(0);
    assert_eq!(view.helpers[0].needs_you, Some(HelperNeeds::SignIn));
    assert_eq!(view.told.len(), 1);
    assert!(view.told[0].text.contains("sign in"), "{:?}", view.told);
    for word in ["ACP", "authenticate", "MCP"] {
        assert!(!view.told[0].text.contains(word), "{:?}", view.told);
    }

    // The card offers the app's own names for the ways.
    let card = hub.card(&coder).expect("card");
    assert_eq!(card.login, LoginShown::NeedsSignIn);
    assert_eq!(card.jobs[0].state, JobShown::NeedsSignIn);
    let names: Vec<_> = card.sign_in.iter().map(|c| c.name.as_str()).collect();
    assert_eq!(
        names,
        ["Sign in with your Claude account", "Sign in in a terminal"]
    );
}

#[tokio::test]
async fn choosing_an_agent_way_signs_in_and_the_job_proceeds() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![
            AgentOutcome::NeedsSignIn(ways()),
            done("Signed in, and done."),
        ],
    );
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::SignedIn);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;
    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    let reply = hub.sign_in(coder.clone(), "claude-login".to_owned()).await;
    assert_eq!(reply, HelperReply::Done);
    assert_eq!(
        apps.signed_with.lock().expect("ways").clone(),
        ["claude-login"]
    );
    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
    assert_eq!(hub.card(&coder).expect("card").login, LoginShown::SignedIn);

    // A way that is not on the card is not run.
    let refused = hub.sign_in(coder.clone(), "made-up".to_owned()).await;
    assert!(matches!(refused, HelperReply::Refused(_)), "{refused:?}");
}

#[tokio::test]
async fn a_terminal_way_asks_the_shell_to_open_one_and_resuming_asks_the_app_again() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![AgentOutcome::NeedsSignIn(ways()), done("Back to it.")],
    );
    let login = TerminalLogin {
        args: vec!["/login".to_owned()],
        env: BTreeMap::new(),
        run: None,
    };
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::OpenTerminal(login.clone()));
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let job = give(&hub, &coder, "compare the three offers").await;
    until("the helper waits", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    let reply = hub
        .sign_in(coder.clone(), "terminal-login".to_owned())
        .await;
    assert_eq!(
        reply,
        HelperReply::OpenTerminal(TerminalRequest {
            helper: coder.clone(),
            program: program(),
            run: login.run.clone(),
            login,
            handoff: None,
        })
    );
    // Nothing is signed in yet and the job still waits.
    assert_eq!(status_of(&hub, job), JobStatus::Blocked(Need::SignIn));

    // The terminal closed: the person says try again, and the app is asked once more.
    hub.resume(coder.clone()).await;
    until("the job is done", || {
        matches!(status_of(&hub, job), JobStatus::Done(_))
    })
    .await;
}

#[tokio::test]
async fn one_login_serves_every_helper_that_runs_the_same_app() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![AgentOutcome::NeedsSignIn(ways()), done("One.")],
    );
    apps.will_run(
        "Reviewer",
        vec![AgentOutcome::NeedsSignIn(ways()), done("Two.")],
    );
    apps.sign_ins
        .lock()
        .expect("sign ins")
        .push_back(SignInOutcome::SignedIn);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let reviewer = make(&hub, "Reviewer").await;
    let first = give(&hub, &coder, "write it").await;
    let second = give(&hub, &reviewer, "read it").await;
    until("both wait", || {
        state_of(&hub, &coder) == HelperState::Waiting(Need::SignIn)
            && state_of(&hub, &reviewer) == HelperState::Waiting(Need::SignIn)
    })
    .await;

    // Signing in for one releases the other: the app has one login.
    assert_eq!(
        hub.sign_in(coder.clone(), "claude-login".to_owned()).await,
        HelperReply::Done
    );
    until("both jobs are done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
            && matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
}

#[tokio::test]
async fn a_crash_ends_that_job_failed_and_open_again_and_touches_no_other_helper() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![AgentOutcome::Failed(JobFault::Crashed)]);
    apps.will_run("Reviewer", vec![done("Fine.")]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let reviewer = make(&hub, "Reviewer").await;
    let broken = give(&hub, &coder, "write it").await;
    let fine = give(&hub, &reviewer, "read it").await;

    until("the other job is done", || {
        matches!(status_of(&hub, fine), JobStatus::Done(_))
    })
    .await;
    until("the crashed job is open again", || {
        let job = hub.board().job(broken).cloned().expect("job");
        job.status == JobStatus::Open && job.failed == Some(JobFault::Crashed)
    })
    .await;
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);
    assert_eq!(hub.answer_words(fine).as_deref(), Some("Fine."));
}

#[tokio::test]
async fn a_conversation_opens_in_the_terminal_pauses_its_helper_and_comes_back() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run(
        "Coder",
        vec![done_resumable("First."), done_resumable("Second.")],
    );
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    // Nothing to take along before a job has run.
    let early = hub.open_in_terminal(coder.clone()).await;
    assert!(
        matches!(&early, HelperReply::Refused(why) if why.contains("Give it a job first")),
        "{early:?}"
    );
    let first = give(&hub, &coder, "compare the offers").await;
    until("the job is done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
    })
    .await;
    let card = hub.card(&coder).expect("card");
    assert_eq!(
        (card.runs_in, card.handover),
        (RunsIn::Desktop, Handover::ToTerminal)
    );

    // Open in Terminal: the window to open carries the resume line and names the session.
    let hand = terminal_hand();
    apps.to_terminal
        .lock()
        .expect("to terminal")
        .push_back(ToTerminalOutcome::Ready(Box::new(hand.clone())));
    let reply = hub.open_in_terminal(coder.clone()).await;
    assert_eq!(
        reply,
        HelperReply::OpenTerminal(TerminalRequest {
            helper: coder.clone(),
            program: program(),
            login: TerminalLogin {
                run: Some(Box::new(hand.run.clone())),
                ..TerminalLogin::default()
            },
            run: Some(Box::new(hand.run)),
            handoff: Some(hand.session),
        })
    );
    let away = hub.card(&coder).expect("card");
    assert_eq!(
        (away.runs_in, away.handover),
        (RunsIn::Terminal, Handover::BringBack)
    );
    assert_eq!(away.line.doing, "In its own terminal window");
    assert_eq!(hub.view(0).helpers[0].doing, "In its own terminal window");

    // A job given meanwhile waits: no job reaches the app while the person works in it.
    let second = give(&hub, &coder, "and the best one?").await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(status_of(&hub, second), JobStatus::Open);
    let again = hub.open_in_terminal(coder.clone()).await;
    assert!(
        matches!(&again, HelperReply::Refused(why) if why.contains("already in the terminal")),
        "{again:?}"
    );

    // Bring Back: the helper goes on and takes the job that waited.
    apps.back.lock().expect("back").push_back(BackOutcome::Back);
    assert_eq!(hub.bring_back(coder.clone()).await, HelperReply::Done);
    until("the second job is done", || {
        matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
    assert_eq!(hub.card(&coder).expect("card").runs_in, RunsIn::Desktop);
}

#[tokio::test]
async fn a_conversation_that_cannot_move_stays_put_and_says_why_in_plain_words() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    apps.will_run("Coder", vec![done_resumable("First."), done("No id.")]);
    let hub = hub(&w, &apps);
    let coder = make(&hub, "Coder").await;
    let first = give(&hub, &coder, "compare the offers").await;
    until("the job is done", || {
        matches!(status_of(&hub, first), JobStatus::Done(_))
    })
    .await;

    // The app says it cannot pick the conversation up: the helper keeps working here.
    apps.to_terminal
        .lock()
        .expect("to terminal")
        .push_back(ToTerminalOutcome::Refused(HandFault::NotResumable));
    let refused = hub.open_in_terminal(coder.clone()).await;
    assert_eq!(
        refused,
        HelperReply::Refused("This app can't pick its conversation back up here.".to_owned())
    );
    let card = hub.card(&coder).expect("card");
    assert_eq!(
        (card.runs_in, card.handover),
        (RunsIn::Desktop, Handover::ToTerminal)
    );
    assert_eq!(state_of(&hub, &coder), HelperState::Idle);

    // Bringing back what is not in the terminal, and one the app would not pick up, are refused.
    let not_there = hub.bring_back(coder.clone()).await;
    assert!(
        matches!(&not_there, HelperReply::Refused(_)),
        "{not_there:?}"
    );

    // A job whose app gave no conversation id of its own leaves nothing to take along.
    let second = give(&hub, &coder, "once more").await;
    until("the job is done", || {
        matches!(status_of(&hub, second), JobStatus::Done(_))
    })
    .await;
    assert_eq!(
        hub.card(&coder).expect("card").handover,
        Handover::Unavailable
    );
    let none = hub.open_in_terminal(coder.clone()).await;
    assert!(matches!(&none, HelperReply::Refused(_)), "{none:?}");

    // A helper that runs no app has no program to open.
    let draft = CardDraft {
        kind: HelperKind::Native,
        ..app_draft("Research")
    };
    let effects = hub.apply(BoardEvent::Create(Box::new(draft))).await;
    let research = effects
        .iter()
        .find_map(|e| match e {
            BoardEffect::Created(id) => Some(id.clone()),
            _ => None,
        })
        .expect("a card was made");
    let plain = hub.open_in_terminal(research).await;
    assert_eq!(
        plain,
        HelperReply::Refused("This helper does not run an app.".to_owned())
    );
}

#[tokio::test]
async fn a_conversation_a_terminal_started_is_listed_and_brought_into_docket() {
    let w = world(vec![]);
    let apps = Arc::new(Apps::default());
    let hub = hub(&w, &apps);
    // A terminal that names its agent app and the app's own id for the conversation.
    let temor = Intents::over(InProcess::new(
        w.router.clone(),
        caller("org.quire.Temor", CallerRole::Cli),
    ));
    let named = temor
        .checkpoint_watch_agent(
            Workspace::parse("/work/project").expect("workspace"),
            "claude in pane 2".to_owned(),
            Rewind::Agent,
            WatchedAgent {
                program: ProgramName::parse("claude-code").expect("program"),
                session: "0a1b-2c3d".to_owned(),
            },
        )
        .await
        .expect("watching");
    // One that does not name its agent is not offered.
    temor
        .checkpoint_watch(
            Workspace::parse("/work/other").expect("workspace"),
            "something".to_owned(),
            Rewind::Docket,
        )
        .await
        .expect("watching");

    let listed = hub.terminal_sessions().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].session, named);
    assert_eq!(listed[0].app_name.as_deref(), Some("Claude Code"));
    assert_eq!(listed[0].folder.as_deref(), Some("/work/project"));
    assert_eq!(listed[0].handover, Handover::BringBack);

    apps.brought
        .lock()
        .expect("brought")
        .push_back(BroughtOutcome::Brought(named.clone()));
    assert_eq!(
        hub.bring_back_terminal(named.clone()).await,
        BroughtBack::Session(named.clone())
    );
    assert_eq!(
        apps.brought_with.lock().expect("brought with").clone(),
        [(
            named,
            "0a1b-2c3d".to_owned(),
            Workspace::parse("/work/project").expect("workspace")
        )]
    );
    // One that is not listed is not there.
    let missing = hub
        .bring_back_terminal(SessionId::parse("s-404").expect("session"))
        .await;
    assert!(matches!(missing, BroughtBack::Refused(_)), "{missing:?}");
}
