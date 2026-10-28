//! Registering, and the agent's own login: asked by accountd, run visibly, reported coarsely.
//! The real command runner is tried on harmless commands in a scratch directory with an
//! unreachable bus and a scratch home.

use super::support::{ENDPOINT, LOGIN};
use docket_launch::fake::{Call, FakeAccounts};
use docket_launch::login::{LoginRunner, VisibleLogin};
use docket_launch::{AgentsFile, AskKind, Entry, Heard, LoginAsk, Registry, Supervisor};
use porter_capability::{LoginFault, LoginOutcome, LoginRequestId};
use porter_core::AccountId;
use std::sync::{Arc, Mutex};

/// A runner that answers from a script and remembers what it was asked.
#[derive(Debug, Clone)]
pub struct Scripted {
    outcome: LoginOutcome,
    ran: Arc<Mutex<Vec<(String, AskKind)>>>,
}

impl Scripted {
    pub fn ready() -> Self {
        Self::with(LoginOutcome::Ready)
    }

    pub fn with(outcome: LoginOutcome) -> Self {
        Self {
            outcome,
            ran: Arc::new(Mutex::new(Vec::new())),
        }
    }
}

impl LoginRunner for Scripted {
    async fn run(&self, entry: &Entry, kind: AskKind) -> LoginOutcome {
        self.ran
            .lock()
            .expect("lock")
            .push((entry.name.as_str().to_owned(), kind));
        self.outcome
    }
}

fn ask(kind: AskKind, program: &str) -> LoginAsk {
    LoginAsk {
        kind,
        request: LoginRequestId::parse("req-1").expect("request"),
        account: AccountId::parse("claude-code").expect("account"),
        program: porter_capability::AgentProgram::parse(program).expect("program"),
    }
}

#[tokio::test]
async fn the_launcher_registers_every_listed_program() {
    let accounts = FakeAccounts::new();
    let file = Arc::new(
        AgentsFile::parse(&format!(
            "{LOGIN}{}",
            ENDPOINT.replace("claude-code", "other-agent")
        ))
        .expect("file"),
    );
    let supervisor = Supervisor::new(
        Arc::new(accounts.clone()),
        file,
        Registry::<docket_launch::fake::FakeProc>::default(),
        Scripted::ready(),
    );
    supervisor.register().await.expect("registered");
    assert_eq!(
        accounts.calls(),
        [Call::Register(vec![
            "claude-code".into(),
            "other-agent".into()
        ])]
    );
}

#[tokio::test]
async fn a_login_request_runs_the_agents_own_command_and_reports_only_the_outcome() {
    let accounts = FakeAccounts::new();
    let file = Arc::new(AgentsFile::parse(LOGIN).expect("file"));
    let runner = Scripted::with(LoginOutcome::Failed(LoginFault::Unreachable));
    let supervisor = Supervisor::new(
        Arc::new(accounts.clone()),
        file,
        Registry::<docket_launch::fake::FakeProc>::default(),
        runner.clone(),
    );
    accounts.say(Heard::Ask(ask(AskKind::Login, "claude-code")));
    accounts.say(Heard::Ask(ask(AskKind::Logout, "claude-code")));
    accounts.say(Heard::Ask(ask(AskKind::Login, "never-listed")));
    supervisor.run().await;
    assert_eq!(
        *runner.ran.lock().expect("lock"),
        [
            ("claude-code".to_owned(), AskKind::Login),
            ("claude-code".to_owned(), AskKind::Logout)
        ]
    );
    let reports: Vec<_> = accounts
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Report(_, outcome) => Some(outcome),
            _ => None,
        })
        .collect();
    assert_eq!(
        reports,
        [
            LoginOutcome::Failed(LoginFault::Unreachable),
            LoginOutcome::Failed(LoginFault::Unreachable),
            // A program we never listed: not installed here, and no command was run for it.
            LoginOutcome::Failed(LoginFault::NotInstalled),
        ]
    );
}

/// A login the person never finishes.
struct Hangs;

impl LoginRunner for Hangs {
    async fn run(&self, _entry: &Entry, _kind: AskKind) -> LoginOutcome {
        std::future::pending().await
    }
}

#[tokio::test]
async fn a_login_nobody_finishes_neither_blocks_the_loop_nor_outlives_its_deadline() {
    let accounts = FakeAccounts::new();
    let file = Arc::new(AgentsFile::parse(LOGIN).expect("file"));
    let supervisor = Supervisor::new(
        Arc::new(accounts.clone()),
        file,
        Registry::<docket_launch::fake::FakeProc>::default(),
        Hangs,
    )
    .with_login_span(std::time::Duration::from_millis(20));
    accounts.say(Heard::Ask(ask(AskKind::Login, "claude-code")));
    // The step returns while the login is still going: whatever is heard next is not behind it.
    assert!(supervisor.step().await);
    supervisor.run().await;
    let reports: Vec<_> = accounts
        .calls()
        .into_iter()
        .filter_map(|c| match c {
            Call::Report(_, outcome) => Some(outcome),
            _ => None,
        })
        .collect();
    assert_eq!(reports, [LoginOutcome::Failed(LoginFault::TimedOut)]);
}

fn entry_with(login: &str, logout: &str) -> Entry {
    let text = format!(
        "[[agent]]\nprogram = \"a\"\ncommand = \"/usr/bin/a\"\nroute = \"login\"\nlogin = {login}\nlogout = {logout}\n"
    );
    let file = AgentsFile::parse(&text).expect("file");
    file.by_program(&porter_capability::AgentProgram::parse("a").expect("p"))
        .expect("entry")
        .clone()
}

#[tokio::test]
async fn the_real_runner_reports_how_the_command_ended_and_gives_it_only_what_it_was_given() {
    let scratch = tempfile::tempdir().expect("scratch");
    let seen = scratch.path().join("env.txt");
    let env = vec![
        (
            "HOME".to_owned(),
            scratch.path().to_str().expect("utf8").to_owned(),
        ),
        ("PATH".to_owned(), "/usr/bin:/bin".to_owned()),
        // An unreachable bus: the command could not reach the real session even if it tried.
        (
            "DBUS_SESSION_BUS_ADDRESS".to_owned(),
            "unix:path=/nonexistent/bus".to_owned(),
        ),
    ];
    let runner = VisibleLogin::new(env);
    let dump = format!(
        "[\"sh\", \"-c\", \"env > {}\"]",
        seen.to_str().expect("utf8")
    );
    let entry = entry_with(&dump, "[\"false\"]");
    assert_eq!(
        runner.run(&entry, AskKind::Login).await,
        LoginOutcome::Ready
    );
    let dumped = std::fs::read_to_string(&seen).expect("dump");
    let mut names: Vec<&str> = dumped.lines().filter_map(|l| l.split('=').next()).collect();
    names.sort_unstable();
    names.retain(|n| !matches!(*n, "PWD" | "SHLVL" | "_" | "OLDPWD"));
    assert_eq!(names, ["DBUS_SESSION_BUS_ADDRESS", "HOME", "PATH"]);

    assert_eq!(
        runner.run(&entry, AskKind::Logout).await,
        LoginOutcome::Failed(LoginFault::Refused)
    );
    let missing = entry_with("[\"/nonexistent/agent\", \"login\"]", "[]");
    assert_eq!(
        runner.run(&missing, AskKind::Login).await,
        LoginOutcome::Failed(LoginFault::NotInstalled)
    );
    assert_eq!(
        runner.run(&missing, AskKind::Logout).await,
        LoginOutcome::Failed(LoginFault::NotInstalled),
        "an entry with no logout command"
    );
}

#[test]
fn a_sign_in_in_a_terminal_runs_the_entrys_command_with_the_login_added_and_only_for_a_login_route()
{
    let file = AgentsFile::parse(LOGIN).expect("file");
    let name = docket_permissions_core::ProgramName::parse("claude-code").expect("name");
    let entry = file.get(&name).expect("entry");
    let folder = bulkhead::AbsPath::parse("/home/me/helpers/h-1").expect("folder");
    let env = std::collections::BTreeMap::from([("BROWSER".to_owned(), "none".to_owned())]);
    let run = docket_launch::login_run(entry, &["/login".to_owned()], &env, &folder)
        .expect("a login entry has a run");
    let words = run.argv.words();
    assert_eq!(words[0], "/home/me/.local/bin/claude-agent-acp");
    assert_eq!(words.last().map(String::as_str), Some("/login"));
    let var = |name: &str| {
        run.env
            .iter()
            .find(|v| v.name == name)
            .map(|v| v.value.as_str())
    };
    assert_eq!(var("BROWSER"), Some("none"));
    assert_eq!(var("HOME"), Some("/home/me"));
    assert_eq!(var("TERM"), Some("xterm-256color"));
    // An entry whose model comes from elsewhere has no sign-in of its own.
    let other = AgentsFile::parse(ENDPOINT).expect("file");
    let entry = other.get(&name).expect("entry");
    assert!(docket_launch::login_run(entry, &[], &env, &folder).is_none());
}
