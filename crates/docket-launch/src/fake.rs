//! For tests: porter as a script (`FakeAccounts`) and the process seam as a recorder
//! (`FakeProcs`). No bus, no process, no clock.

use crate::accounts::{
    AccountFault, Accounts, Heard, Issued, KeyHandoff, LoginAsk, OpenedEndpoint, RouteWish,
};
use crate::config::Delivery;
use crate::procs::{Proc, ProcFault, Procs};
use bulkhead::AgentRun;
use docket_acp::client::fake::ChannelWire;
use docket_core::AbsPath;
use porter_core::capability::{AgentProgram, AgentProtocol, EnvName};
use porter_core::{
    DataClass, GrantId, LauncherSession, LoginOutcome, ProcessCredentialId, SecretText,
};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

fn locked<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// One call made to the fake porter, in order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Call {
    /// `register`.
    Register(Vec<String>),
    /// `begin_session`.
    Begin(String),
    /// `end_session`.
    End(String),
    /// `open_endpoint`: program and the route's id.
    Open(String, String),
    /// `close_endpoint`.
    Close(String),
    /// `request_grant`.
    Grant(String),
    /// `issue`: program and how it was to be delivered.
    Issue(String, Delivery),
    /// `revoke`.
    Revoke(String),
    /// `report`: the request and the outcome.
    Report(String, LoginOutcome),
}

/// What the fake does when asked for something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    /// Everything works.
    #[default]
    Working,
    /// The endpoint cannot be opened.
    NoEndpoint,
    /// accountd is not there.
    Down,
}

/// The values the fake hands out.
#[derive(Debug, Clone)]
pub struct Secrets {
    /// The per-session token inferd would give.
    pub token: String,
    /// The key a handoff would give.
    pub key: String,
    /// Where a file handoff would put it.
    pub key_file: String,
    /// The endpoint's port.
    pub port: u16,
}

impl Default for Secrets {
    fn default() -> Self {
        Self {
            token: "tok-session-7c1d".to_owned(),
            key: "sk-test-handoff-9f3a".to_owned(),
            key_file: "/run/user/1000/porter/agent/key".to_owned(),
            port: 41999,
        }
    }
}

#[derive(Debug, Default)]
struct State {
    calls: Vec<Call>,
    heard: VecDeque<Heard>,
}

/// A scripted porter.
#[derive(Debug, Clone)]
pub struct FakeAccounts {
    state: Arc<Mutex<State>>,
    mood: Mood,
    secrets: Secrets,
}

impl FakeAccounts {
    /// A porter that works and hands out the default values.
    pub fn new() -> Self {
        Self::with(Mood::Working, Secrets::default())
    }

    /// A porter in `mood` handing out `secrets`.
    pub fn with(mood: Mood, secrets: Secrets) -> Self {
        Self {
            state: Arc::new(Mutex::new(State::default())),
            mood,
            secrets,
        }
    }

    /// What it was asked, in order.
    pub fn calls(&self) -> Vec<Call> {
        locked(&self.state).calls.clone()
    }

    /// Queues something for the launcher to hear.
    pub fn say(&self, heard: Heard) {
        locked(&self.state).heard.push_back(heard);
    }

    fn note(&self, call: Call) -> Result<(), AccountFault> {
        locked(&self.state).calls.push(call);
        match self.mood {
            Mood::Down => Err(AccountFault::Unreachable),
            _ => Ok(()),
        }
    }
}

impl Default for FakeAccounts {
    fn default() -> Self {
        Self::new()
    }
}

impl Accounts for FakeAccounts {
    async fn register(&self, programs: &[AgentProgram]) -> Result<(), AccountFault> {
        self.note(Call::Register(
            programs.iter().map(|p| p.as_str().to_owned()).collect(),
        ))
    }

    async fn begin_session(&self, session: &LauncherSession) -> Result<(), AccountFault> {
        self.note(Call::Begin(session.as_str().to_owned()))
    }

    async fn end_session(&self, session: &LauncherSession) -> Result<(), AccountFault> {
        self.note(Call::End(session.as_str().to_owned()))
    }

    async fn open_endpoint(
        &self,
        program: &AgentProgram,
        route: &RouteWish,
        _class: DataClass,
        _protocol: AgentProtocol,
    ) -> Result<OpenedEndpoint, AccountFault> {
        self.note(Call::Open(program.as_str().to_owned(), route.id.clone()))?;
        if self.mood == Mood::NoEndpoint {
            return Err(AccountFault::Refused);
        }
        Ok(OpenedEndpoint {
            session: "ep-1".to_owned(),
            host: "127.0.0.1".to_owned(),
            port: self.secrets.port,
            base_url: format!("http://127.0.0.1:{}", self.secrets.port),
            token: SecretText::new(self.secrets.token.clone()),
        })
    }

    async fn close_endpoint(&self, session: &str) -> Result<(), AccountFault> {
        self.note(Call::Close(session.to_owned()))
    }

    async fn request_grant(
        &self,
        program: &AgentProgram,
        _class: DataClass,
        _session: &LauncherSession,
    ) -> Result<GrantId, AccountFault> {
        self.note(Call::Grant(program.as_str().to_owned()))?;
        GrantId::parse("grant-1").map_err(|_| AccountFault::Other)
    }

    async fn issue(
        &self,
        _grant: &GrantId,
        program: &AgentProgram,
        _key_env: &EnvName,
        delivery: Delivery,
    ) -> Result<Issued, AccountFault> {
        self.note(Call::Issue(program.as_str().to_owned(), delivery))?;
        let key = match delivery {
            Delivery::Value => KeyHandoff::Value(SecretText::new(self.secrets.key.clone())),
            Delivery::File => KeyHandoff::File(
                AbsPath::parse(&self.secrets.key_file).map_err(|_| AccountFault::Other)?,
            ),
        };
        Ok(Issued {
            id: ProcessCredentialId::parse("cred-1").map_err(|_| AccountFault::Other)?,
            key,
        })
    }

    async fn revoke(&self, id: &ProcessCredentialId) -> Result<(), AccountFault> {
        self.note(Call::Revoke(id.as_str().to_owned()))
    }

    async fn hear(&self) -> Option<Heard> {
        locked(&self.state).heard.pop_front()
    }

    async fn report(&self, ask: &LoginAsk, outcome: LoginOutcome) -> Result<(), AccountFault> {
        self.note(Call::Report(ask.request.as_str().to_owned(), outcome))
    }
}

/// The recorder's view of what ran.
#[derive(Debug, Default)]
struct Ran {
    runs: Vec<AgentRun>,
    killed: Vec<bool>,
}

/// A shared view of the fake process seam.
#[derive(Debug, Clone, Default)]
pub struct ProcsSeen(Arc<Mutex<Ran>>);

impl ProcsSeen {
    /// Every run it was asked to start.
    pub fn runs(&self) -> Vec<AgentRun> {
        locked(&self.0).runs.clone()
    }

    /// Whether the nth process was killed.
    pub fn killed(&self, n: usize) -> bool {
        locked(&self.0).killed.get(n).copied().unwrap_or(false)
    }
}

/// The fake's process handle.
#[derive(Debug)]
pub struct FakeProc {
    seen: ProcsSeen,
    index: usize,
}

impl Proc for FakeProc {
    fn kill(&mut self) {
        if let Some(k) = locked(&self.seen.0).killed.get_mut(self.index) {
            *k = true;
        }
    }
}

/// A `Procs` that records each run and returns the wires it was given.
#[derive(Debug)]
pub struct FakeProcs {
    wires: VecDeque<ChannelWire>,
    seen: ProcsSeen,
}

impl FakeProcs {
    /// Starts return `wires` in order; the view shows what was asked.
    pub fn new(wires: Vec<ChannelWire>) -> (Self, ProcsSeen) {
        let seen = ProcsSeen::default();
        (
            Self {
                wires: wires.into(),
                seen: seen.clone(),
            },
            seen,
        )
    }
}

impl Procs for FakeProcs {
    type Wire = ChannelWire;
    type Proc = FakeProc;

    async fn start(&mut self, run: &AgentRun) -> Result<(ChannelWire, FakeProc), ProcFault> {
        let wire = self.wires.pop_front().ok_or(ProcFault::Spawn)?;
        let index = {
            let mut ran = locked(&self.seen.0);
            ran.runs.push(run.clone());
            ran.killed.push(false);
            ran.runs.len() - 1
        };
        Ok((
            wire,
            FakeProc {
                seen: self.seen.clone(),
                index,
            },
        ))
    }
}
