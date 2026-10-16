//! `docket-agent --serve`: the host as a service, which companiond asks to give the person's turn
//! to the coding agent they chose as the assistant's model (`org.quire.AgentHost1.HandOver`).
//!
//! This process, not companiond, is the agent's host: it plays `org.quire.AcpAgent`, so it alone
//! opens the agent's session at the router (the router lets only that role do so), records the
//! person's turn there, and makes every call the agent asks of it as a router call, ruled and
//! asked about exactly as for `docket-agent` on a terminal. Companiond opens no session of an
//! outside agent and gains no way to; the only thing it can ask here is a turn.
//!
//! It is started by the bus when companiond first calls (`org.quire.AcpAgent` is activatable),
//! holds one agent's session open between turns so the conversation goes on, and ends after a
//! quiet while. Handing a turn to another agent ends the process and the next call starts it
//! again, because the host serves one agent's pseudo-app at a time.
//!
//! Only companiond may call: the callers table (`acp-callers.toml`) names its unit.

use super::fault::AgentFault;
use super::host::{HostError, Hosted, host_typed};
use super::wire::{self, Tools};
use super::{data_dir, env};
use docket_acp::client::{Fallback, Workdir};
use docket_core::{AbsPath, HandOver, HandRefusal, HandReply, Handed, Reveal, UserTurn};
use docket_dbus::callers::{COMPANIOND, Identity, require};
use docket_dbus::{AGENT_HOST_BUS, AGENT_HOST_PATH, BusConnection};
use docket_launch::dbus::DbusAccounts;
use docket_session::{
    BackendEvent, BackendFault, EndCause, HostFault, ProgramName, SessionHost, TurnEnd,
};
use prov::SpaceId;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Notify, watch};
use zbus::fdo;
use zbus::message::Header;

/// How long the host keeps an agent's session open with nothing asked of it.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How often the host looks at the clock for that.
const WATCH: Duration = Duration::from_secs(20);

/// How long one `Wait` holds a call before telling the caller to ask again: well under what the
/// bus lets a call wait for its reply.
const WAIT: Duration = Duration::from_secs(60);

/// The turns started and not yet collected; the oldest are forgotten beyond this.
const KEPT: usize = 8;

/// The turn asked about was never started, or was collected already.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("no such turn")]
pub struct NoSuchTurn;

/// The folder an agent works in when the assistant hands it a turn: its own, under docket's data
/// directory, never the person's home or a project. `None` for a name that is not a plain slug,
/// which cannot be the name of a folder.
pub fn workspace_name(program: &ProgramName) -> Option<&str> {
    let name = program.as_str();
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'));
    (plain && !name.starts_with('.')).then_some(name)
}

/// What a setup fault is to the person who asked.
pub fn refusal_of_fault(fault: &AgentFault) -> HandRefusal {
    match fault {
        AgentFault::Permit(_) => HandRefusal::TurnedOff,
        AgentFault::NoAgentsFile => HandRefusal::NotSetUp,
        AgentFault::Agents(_) => HandRefusal::Failed {
            message: "The list of coding agents can't be read. Open Settings to check it."
                .to_owned(),
        },
        AgentFault::NoSandbox => HandRefusal::Failed {
            message: "This computer has no sandbox, so no coding agent is started.".to_owned(),
        },
        _ => HandRefusal::Failed {
            message: "The coding agent could not be started.".to_owned(),
        },
    }
}

/// What a session that did not open is to the person who asked.
pub fn refusal_of_open(error: HostError, label: &str) -> HandRefusal {
    match error {
        HostError::Open(HostFault::Backend(
            BackendFault::SignInNeeded
            | BackendFault::SignInUnsupported
            | BackendFault::SignInChoose(_),
        )) => HandRefusal::NeedsSignIn {
            label: label.to_owned(),
        },
        HostError::Open(HostFault::Backend(
            BackendFault::ModelNotOffered(_)
            | BackendFault::ModelNotChoosable
            | BackendFault::ModelRefused,
        )) => HandRefusal::Failed {
            message: format!(
                "The model chosen for {label} is not available. Choose another in Settings."
            ),
        },
        HostError::Setup(words) => HandRefusal::Failed { message: words },
        HostError::Open(_) => HandRefusal::Failed {
            message: format!("{label} could not be started."),
        },
    }
}

/// What the turn's end is to the person who asked: the agent's words, or why there are none.
pub fn end_of(end: TurnEnd, said: &[String], label: &str) -> Result<Handed, HandRefusal> {
    let failed = |message: String| Err(HandRefusal::Failed { message });
    match end {
        TurnEnd::Done => {
            let words = said.concat();
            let words = match words.trim() {
                "" => format!("{label} finished without a written answer."),
                text => text.to_owned(),
            };
            Ok(Handed {
                words,
                label: label.to_owned(),
            })
        }
        TurnEnd::Cancelled => failed(format!("{label} was stopped.")),
        TurnEnd::Paused(_) => failed(format!("{label} paused and is waiting for you.")),
        TurnEnd::Failed | TurnEnd::Refused => failed(format!("{label} could not finish.")),
    }
}

/// Whether an agent's session can take another turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    /// It can.
    Sound,
    /// It stopped answering: the host ends, and the bus starts it again for the next turn.
    Broken,
}

/// One agent's open session.
struct Active {
    program: ProgramName,
    label: String,
    hosted: Hosted<DbusAccounts>,
    health: Health,
}

enum State {
    /// Nothing started.
    Fresh,
    /// One agent's session is open.
    Active(Box<Active>),
}

/// The service: who may call it, the agent's session, and when it was last used.
pub struct Service {
    bus: BusConnection,
    state: Mutex<State>,
    last: std::sync::Mutex<Instant>,
    done: Notify,
    turns: std::sync::Mutex<BTreeMap<u64, watch::Receiver<Option<HandReply>>>>,
    next: AtomicU64,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service")
    }
}

impl Service {
    /// A service with nothing started, on `bus`.
    pub fn new(bus: BusConnection) -> Self {
        Self {
            bus,
            state: Mutex::new(State::Fresh),
            last: std::sync::Mutex::new(Instant::now()),
            done: Notify::new(),
            turns: std::sync::Mutex::new(BTreeMap::new()),
            next: AtomicU64::new(0),
        }
    }

    /// Starts handing `ask` over and answers the turn's id; [`Self::wait`] collects the reply.
    pub fn begin(self: &Arc<Self>, ask: HandOver) -> String {
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let (done, reply) = watch::channel(None);
        if let Ok(mut turns) = self.turns.lock() {
            turns.insert(id, reply);
            while turns.len() > KEPT {
                turns.pop_first();
            }
        }
        let service = Arc::clone(self);
        tokio::spawn(async move {
            let reply = service.hand_over(ask).await;
            // The caller stopped asking after it; nothing to tell.
            let _ = done.send(Some(reply));
        });
        id.to_string()
    }

    /// The reply of the turn `id` once it has ended, waiting up to [`WAIT`] for it: `None` while
    /// the agent is still working. A turn's reply is given once.
    pub async fn wait(&self, id: &str) -> Result<Option<HandReply>, NoSuchTurn> {
        let id: u64 = id.parse().map_err(|_| NoSuchTurn)?;
        let mut reply = self
            .turns
            .lock()
            .ok()
            .and_then(|turns| turns.get(&id).cloned())
            .ok_or(NoSuchTurn)?;
        let ended = tokio::time::timeout(WAIT, reply.wait_for(Option::is_some)).await;
        let collected = match ended {
            Ok(Ok(value)) => value.clone(),
            // The turn's task ended without a reply: it stopped answering.
            Ok(Err(_)) => Some(HandReply::Refused(HandRefusal::Failed {
                message: "The coding agent stopped answering.".to_owned(),
            })),
            Err(_) => None,
        };
        if collected.is_some()
            && let Ok(mut turns) = self.turns.lock()
        {
            turns.remove(&id);
        }
        Ok(collected)
    }

    fn touch(&self) {
        if let Ok(mut last) = self.last.lock() {
            *last = Instant::now();
        }
    }

    fn quiet_for(&self) -> Duration {
        self.last
            .lock()
            .map_or(Duration::ZERO, |last| last.elapsed())
    }

    /// The turn handed over: the agent's answer, or why it was not given.
    pub async fn hand_over(&self, ask: HandOver) -> HandReply {
        let mut state = self.state.lock().await;
        self.touch();
        let reply = self.take(&mut state, ask).await;
        self.touch();
        match reply {
            Ok(handed) => HandReply::Handed(handed),
            Err(refusal) => HandReply::Refused(refusal),
        }
    }

    async fn take(&self, state: &mut State, ask: HandOver) -> Result<Handed, HandRefusal> {
        let HandOver {
            program,
            space,
            turn,
        } = ask;
        let switching = matches!(&*state, State::Active(held) if held.program != program);
        if switching {
            // The pseudo-app is served for one agent at a time: this one ends, and the bus
            // starts the host again for the next.
            self.finish(state).await;
            return Err(HandRefusal::Switching);
        }
        if matches!(*state, State::Fresh) {
            *state = State::Active(Box::new(self.start(&program, space).await?));
        }
        let State::Active(active) = state else {
            return Err(HandRefusal::Switching);
        };
        let answered = active.run(turn).await;
        if active.health == Health::Broken {
            self.finish(state).await;
        }
        answered
    }

    /// Starts the agent `program` for the assistant: the setting and `agents.toml` allow it, its
    /// folder is its own, its session opens at the router as this process's role.
    async fn start(&self, program: &ProgramName, space: SpaceId) -> Result<Active, HandRefusal> {
        let allowed = wire::allowed().map_err(|fault| refusal_of_fault(&fault))?;
        let entry = allowed.file.get(program).ok_or(HandRefusal::NotSetUp)?;
        let label = entry
            .label
            .as_ref()
            .map_or_else(|| program.as_str().to_owned(), |label| label.0.clone());
        let cwd = folder(program).map_err(|why| HandRefusal::Failed { message: why })?;
        let wiring = wire::build(
            allowed,
            program,
            Tools::Wanted,
            Fallback::Off,
            self.bus.clone(),
        )
        .await
        .map_err(|fault| refusal_of_fault(&fault))?;
        let hosted = host_typed(wiring, program.clone(), &cwd, space)
            .await
            .map_err(|error| refusal_of_open(error, &label))?;
        Ok(Active {
            program: program.clone(),
            label,
            hosted,
            health: Health::Sound,
        })
    }

    /// Closes the agent's session, if one is open, and asks the process to end.
    async fn finish(&self, state: &mut State) {
        if let State::Active(active) = std::mem::replace(state, State::Fresh) {
            let Active { hosted, .. } = *active;
            let Hosted { mut host, session } = hosted;
            // The router hears the session end; an agent that is already gone has nothing to say.
            let _ = host.close(&session, EndCause::Closed).await;
        }
        self.done.notify_one();
    }

    /// Returns when the host should end: it asked to, or nothing has been asked of it for
    /// [`IDLE`] and no turn is running.
    pub async fn until_done(&self) {
        loop {
            tokio::select! {
                () = self.done.notified() => return,
                () = tokio::time::sleep(WATCH) => {
                    if self.quiet_for() > IDLE && self.state.try_lock().is_ok() {
                        return;
                    }
                }
            }
        }
    }

    /// Waits a moment for the replies nobody has collected yet, so a caller still asking is not
    /// left with a host that is gone: at most ten seconds.
    pub async fn drain(&self) {
        for _ in 0..20 {
            if self.turns.lock().map_or(true, |turns| turns.is_empty()) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Closes the session still open, on the way out.
    pub async fn close(&self) {
        let mut state = self.state.lock().await;
        self.finish(&mut state).await;
    }
}

impl Active {
    /// Gives the agent the person's turn and collects what it says until the turn ends.
    async fn run(&mut self, turn: UserTurn) -> Result<Handed, HandRefusal> {
        let label = self.label.clone();
        let broke = |label: &str| HandRefusal::Failed {
            message: format!("{label} stopped answering."),
        };
        let Hosted { host, session } = &mut self.hosted;
        if host.turn(session, turn).await.is_err() {
            self.health = Health::Broken;
            return Err(broke(&label));
        }
        let mut said = Vec::new();
        loop {
            match host.next_event(session).await {
                Ok(Some(BackendEvent::Words(Reveal::Plain(text)))) => said.push(text),
                Ok(Some(BackendEvent::TurnEnd(end))) => return end_of(end, &said, &label),
                // Calls, thoughts and usage are the router's to rule and record; handles the
                // agent names stay in its session.
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => {
                    self.health = Health::Broken;
                    return Err(broke(&label));
                }
            }
        }
    }
}

/// The folder the agent works in, made if it is not there.
fn folder(program: &ProgramName) -> Result<String, String> {
    let name = workspace_name(program)
        .ok_or_else(|| "That coding agent can't be used by the assistant.".to_owned())?;
    let dir: PathBuf = data_dir().join("assistant").join(name);
    std::fs::create_dir_all(&dir).map_err(|_| "The agent's folder can't be made.".to_owned())?;
    let real =
        std::fs::canonicalize(&dir).map_err(|_| "The agent's folder can't be used.".to_owned())?;
    let text = real
        .to_str()
        .ok_or_else(|| "The agent's folder can't be used.".to_owned())?
        .to_owned();
    let path = AbsPath::parse(&text).map_err(|_| "The agent's folder can't be used.".to_owned())?;
    let home = env("HOME").map(PathBuf::from);
    Workdir::resolve(&path, &path, home.as_deref())
        .map_err(|_| "The agent's folder can't be used.".to_owned())?;
    Ok(text)
}

/// The object companiond calls.
struct Object {
    service: Arc<Service>,
    callers: Arc<Identity>,
}

#[zbus::interface(name = "org.quire.AgentHost1")]
impl Object {
    async fn hand_over(
        &self,
        handover: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        let ask: HandOver =
            serde_json::from_str(&handover).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        Ok(self.service.begin(ask))
    }

    async fn wait(&self, turn: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        match self.service.wait(&turn).await {
            Ok(Some(reply)) => {
                serde_json::to_string(&reply).map_err(|e| fdo::Error::Failed(e.to_string()))
            }
            Ok(None) => Ok(String::new()),
            Err(no_such) => Err(fdo::Error::InvalidArgs(no_such.to_string())),
        }
    }
}

/// Serves `AgentHost1` under the host's name until it has been quiet for [`IDLE`].
pub async fn run() -> Result<(), AgentFault> {
    let bus_fault = |e: zbus::Error| AgentFault::Bus(e.to_string());
    let bus = docket_dbus::session_connection(&env)
        .await
        .map_err(bus_fault)?;
    let table = crate::callers::read(&env).map_err(|e| AgentFault::Callers(e.to_string()))?;
    let callers = Arc::new(Identity::on_system(bus.clone(), table));
    let service = Arc::new(Service::new(bus.clone()));
    // Served before the name is claimed, so the first call finds it.
    bus.object_server()
        .at(
            AGENT_HOST_PATH,
            Object {
                service: service.clone(),
                callers,
            },
        )
        .await
        .map_err(bus_fault)?;
    bus.request_name(AGENT_HOST_BUS).await.map_err(bus_fault)?;
    service.until_done().await;
    service.close().await;
    service.drain().await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> ProgramName {
        ProgramName::parse(text).expect("program name")
    }

    #[test]
    fn only_a_plain_slug_is_a_folder_name() {
        let rows = [
            ("claude-code", Some("claude-code")),
            ("gemini_cli.2", Some("gemini_cli.2")),
            (".hidden", None),
            ("a/b", None),
            ("..", None),
            ("two words", None),
        ];
        for (text, expected) in rows {
            assert_eq!(workspace_name(&name(text)), expected, "{text}");
        }
    }

    #[test]
    fn a_setting_that_is_off_and_a_missing_list_are_said_as_what_they_are() {
        let rows = [
            (AgentFault::Permit("off".into()), HandRefusal::TurnedOff),
            (AgentFault::NoAgentsFile, HandRefusal::NotSetUp),
        ];
        for (fault, expected) in rows {
            assert_eq!(refusal_of_fault(&fault), expected, "{fault:?}");
        }
        let said = refusal_of_fault(&AgentFault::NoSandbox).sentence();
        assert!(!said.contains("bubblewrap"), "{said}");
    }

    #[test]
    fn a_missing_sign_in_names_the_agent_and_other_faults_name_no_machinery() {
        let sign_in = HostError::Open(HostFault::Backend(BackendFault::SignInNeeded));
        assert_eq!(
            refusal_of_open(sign_in, "Claude Code"),
            HandRefusal::NeedsSignIn {
                label: "Claude Code".to_owned()
            }
        );
        let down = HostError::Open(HostFault::Backend(BackendFault::Unavailable));
        let said = refusal_of_open(down, "Claude Code").sentence();
        assert_eq!(said, "Claude Code could not be started.");
    }

    #[test]
    fn a_finished_turn_is_the_agents_words_and_the_others_are_why_not() {
        let said = ["Hello".to_owned(), ", there.".to_owned()];
        let handed = end_of(TurnEnd::Done, &said, "Claude Code").expect("handed");
        assert_eq!(handed.words, "Hello, there.");
        assert_eq!(handed.label, "Claude Code");
        let quiet = end_of(TurnEnd::Done, &[], "Claude Code").expect("handed");
        assert_eq!(
            quiet.words,
            "Claude Code finished without a written answer."
        );
        for end in [TurnEnd::Failed, TurnEnd::Refused, TurnEnd::Cancelled] {
            assert!(end_of(end, &said, "Claude Code").is_err(), "{end:?}");
        }
    }
}
