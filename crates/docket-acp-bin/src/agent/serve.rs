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
//! serves the pseudo-app once for every use, holds the assistant's agent session open between
//! turns so the conversation goes on, runs the helpers' agents beside it, and ends after a quiet
//! while. Handing a turn to another agent closes the first one's session and opens the next in the
//! same process. The assistant's turns and every helper's jobs run at the same time; only calls
//! on one session wait for each other.
//!
//! Only companiond may call: the callers table (`acp-callers.toml`) names its unit.

use super::calls::{Calls, NoSuchTurn};
use super::env;
use super::fault::AgentFault;
use super::folders::folder;
use super::hand_words::{end_of, refusal_of_fault, refusal_of_open};
use super::helper_host::HelperHost;
use super::host::{Hosted, open_on};
use super::live::Live;
use super::wire::{self, SandboxLimits, Tools};
use bulkhead::NetworkMode;
use docket_acp_host::Fallback;
use docket_core::helper::{AppsAsk, AppsReply};
use docket_core::{HandOver, HandRefusal, HandReply, Handed, Reveal, UserTurn};
use docket_dbus::callers::{COMPANIOND, Identity, require};
use docket_dbus::{AGENT_HOST_BUS, AGENT_HOST_PATH, BusConnection};
use docket_launch::dbus::DbusAccounts;
use docket_session::{BackendEvent, EndCause, ProgramName, SessionHost};
use prov::SpaceId;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OnceCell};
use zbus::fdo;
use zbus::message::Header;

/// How long the host keeps an agent's session open with nothing asked of it.
pub const IDLE: Duration = Duration::from_secs(10 * 60);

/// How often the host looks at the clock for that.
const WATCH: Duration = Duration::from_secs(20);

/// Whether an agent's session can take another turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Health {
    /// It can.
    Sound,
    /// It stopped answering: its session is closed, and the next turn opens a new one.
    Broken,
}

/// One agent's open session.
struct Active {
    program: ProgramName,
    label: String,
    hosted: Hosted<DbusAccounts>,
    health: Health,
}

/// The service: who may call it, the pseudo-app every use shares, the assistant's agent session,
/// the helpers' agents, and when it was last used.
pub struct Service {
    bus: BusConnection,
    limits: SandboxLimits,
    live: OnceCell<Arc<Live<DbusAccounts>>>,
    assistant: Mutex<Option<Box<Active>>>,
    helpers: OnceLock<Arc<HelperHost>>,
    caller: std::sync::Mutex<Option<String>>,
    last: std::sync::Mutex<Instant>,
    turns: Calls<HandReply>,
    apps: Calls<AppsReply>,
}

impl std::fmt::Debug for Service {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Service")
    }
}

impl Service {
    /// A service with nothing started, on `bus`, whose agents' sandboxes are bounded by `limits`.
    pub fn new(bus: BusConnection, limits: SandboxLimits) -> Self {
        Self {
            bus,
            limits,
            live: OnceCell::new(),
            assistant: Mutex::new(None),
            helpers: OnceLock::new(),
            caller: std::sync::Mutex::new(None),
            last: std::sync::Mutex::new(Instant::now()),
            turns: Calls::default(),
            apps: Calls::default(),
        }
    }

    /// Starts handing `ask` over and answers the turn's id; [`Self::wait`] collects the reply.
    pub fn begin(self: &Arc<Self>, ask: HandOver) -> String {
        let service = Arc::clone(self);
        self.turns
            .begin(async move { service.hand_over(ask).await })
    }

    /// The reply of the turn `id` once it has ended, waiting up to a minute for it: `None` while
    /// the agent is still working. A turn's reply is given once.
    pub async fn wait(&self, id: &str) -> Result<Option<HandReply>, NoSuchTurn> {
        // The turn's task ended without a reply: the agent stopped answering.
        let lost = HandReply::Refused(HandRefusal::Failed {
            message: "The coding agent stopped answering.".to_owned(),
        });
        self.turns.wait(id, lost).await
    }

    /// Starts one call on the helpers' agents and answers its id; [`Self::apps_wait`] collects
    /// the reply.
    pub fn apps_begin(self: &Arc<Self>, ask: AppsAsk) -> String {
        let service = Arc::clone(self);
        self.apps.begin(async move { service.apps(ask).await })
    }

    /// The reply of the call `id` once it has ended: `None` while it is still working.
    pub async fn apps_wait(&self, id: &str) -> Result<Option<AppsReply>, NoSuchTurn> {
        self.apps.wait(id, AppsReply::Forget).await
    }

    /// The call on the helpers' agents. It does not wait for the assistant's turn or another
    /// helper's job; only a call on the same helper's own process queues behind it.
    async fn apps(&self, ask: AppsAsk) -> AppsReply {
        self.touch();
        let reply = match self.helper_host().await {
            Ok(host) => host.ask(ask).await,
            Err(fault) => HelperHost::unavailable(&ask, fault),
        };
        self.touch();
        reply
    }

    /// The pseudo-app and the parts agents are made of, made once for every use: the setting and
    /// `agents.toml` must allow it. The commands an agent runs count against the widest network,
    /// because one host serves the agents of every program.
    async fn live(&self) -> Result<Arc<Live<DbusAccounts>>, AgentFault> {
        let made = self
            .live
            .get_or_try_init(|| async {
                let wiring = wire::build(
                    wire::allowed()?,
                    Tools::Wanted,
                    Fallback::Off,
                    self.bus.clone(),
                    self.limits,
                )
                .await?;
                let live = Live::serve(wiring, NetworkMode::Host)
                    .await
                    .map_err(AgentFault::Bus)?;
                Ok::<_, AgentFault>(Arc::new(live))
            })
            .await?;
        Ok(Arc::clone(made))
    }

    async fn helper_host(&self) -> Result<Arc<HelperHost>, AgentFault> {
        let live = self.live().await?;
        let host = self.helpers.get_or_init(|| Arc::new(HelperHost::new(live)));
        Ok(Arc::clone(host))
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
        self.touch();
        let reply = self.take(ask).await;
        self.touch();
        match reply {
            Ok(handed) => HandReply::Handed(handed),
            Err(refusal) => HandReply::Refused(refusal),
        }
    }

    async fn take(&self, ask: HandOver) -> Result<Handed, HandRefusal> {
        let HandOver {
            program,
            space,
            turn,
        } = ask;
        let live = self
            .live()
            .await
            .map_err(|fault| refusal_of_fault(&fault))?;
        // The assistant has one agent session at a time; its turns wait for each other, never for
        // a helper.
        let mut held = self.assistant.lock().await;
        if held
            .as_ref()
            .is_some_and(|active| active.program != program)
        {
            Self::finish(&mut held).await;
        }
        if held.is_none() {
            *held = Some(Box::new(Self::start(&live, &program, space).await?));
        }
        let Some(active) = held.as_mut() else {
            return Err(HandRefusal::Failed {
                message: "The coding agent could not be started.".to_owned(),
            });
        };
        let answered = active.run(turn).await;
        if active.health == Health::Broken {
            Self::finish(&mut held).await;
        }
        answered
    }

    /// Starts the agent `program` for the assistant: `agents.toml` lists it, its folder is its
    /// own, its session opens at the router as this process's role.
    async fn start(
        live: &Live<DbusAccounts>,
        program: &ProgramName,
        space: SpaceId,
    ) -> Result<Active, HandRefusal> {
        let entry = live.entry(program).ok_or(HandRefusal::NotSetUp)?;
        let label = entry
            .label
            .as_ref()
            .map_or_else(|| program.as_str().to_owned(), |label| label.0.clone());
        let cwd = folder(program).map_err(|why| HandRefusal::Failed { message: why })?;
        let hosted = open_on(live, program.clone(), &cwd, space)
            .await
            .map_err(|error| refusal_of_open(error, &label))?;
        Ok(Active {
            program: program.clone(),
            label,
            hosted,
            health: Health::Sound,
        })
    }

    /// Closes the assistant's agent session, if one is open.
    async fn finish(held: &mut Option<Box<Active>>) {
        if let Some(active) = held.take() {
            let Active { hosted, .. } = *active;
            let Hosted { mut host, session } = hosted;
            // The router hears the session end; an agent that is already gone has nothing to say.
            let _ = host.close(&session, EndCause::Closed).await;
        }
    }

    /// Returns when the host should end: nothing has been asked of it for [`IDLE`] and no turn
    /// is running, or the daemon that asked for the helpers' agents has left.
    pub async fn until_done(&self) {
        loop {
            tokio::time::sleep(WATCH).await;
            if self.quiet_for() > IDLE && self.assistant.try_lock().is_ok() && !self.helpers_busy()
            {
                return;
            }
            if self.caller_gone().await {
                return;
            }
        }
    }

    /// Waits a moment for the replies nobody has collected yet, so a caller still asking is not
    /// left with a host that is gone: at most ten seconds.
    pub async fn drain(&self) {
        for _ in 0..20 {
            if self.turns.is_empty() && self.apps.is_empty() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    /// Closes the session still open and ends the helpers' agents, on the way out.
    pub async fn close(&self) {
        Self::finish(&mut *self.assistant.lock().await).await;
        if let Some(helpers) = self.helpers.get() {
            helpers.shutdown().await;
        }
    }

    /// Whether a call on the helpers' agents is running.
    fn helpers_busy(&self) -> bool {
        self.helpers.get().is_some_and(|helpers| helpers.busy())
    }

    /// Notes who asked for the helpers' agents, to end them when that caller is gone.
    pub fn remember_caller(&self, sender: Option<&str>) {
        if let (Some(sender), Ok(mut caller)) = (sender, self.caller.lock()) {
            *caller = Some(sender.to_owned());
        }
    }

    /// Whether the daemon that asked for the helpers' agents has left the bus: no agent process
    /// outlives it.
    async fn caller_gone(&self) -> bool {
        let Some(name) = self.caller.lock().ok().and_then(|caller| caller.clone()) else {
            return false;
        };
        let (Ok(bus), Ok(name)) = (
            fdo::DBusProxy::new(&self.bus).await,
            zbus::names::BusName::try_from(name),
        ) else {
            return false;
        };
        matches!(bus.name_has_owner(name).await, Ok(false))
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

    async fn apps_begin(
        &self,
        ask: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        let ask: AppsAsk =
            serde_json::from_str(&ask).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        self.service.remember_caller(sender);
        Ok(self.service.apps_begin(ask))
    }

    async fn apps_wait(
        &self,
        call: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<String> {
        let sender = header.sender().map(|s| s.as_str());
        require(&*self.callers, sender, &[COMPANIOND]).await?;
        match self.service.apps_wait(&call).await {
            Ok(Some(reply)) => {
                serde_json::to_string(&reply).map_err(|e| fdo::Error::Failed(e.to_string()))
            }
            Ok(None) => Ok(String::new()),
            Err(no_such) => Err(fdo::Error::InvalidArgs(no_such.to_string())),
        }
    }
}

/// What only the acceptance harness chooses, as docket-acp's `HarnessSeams` does: the `/proc` the
/// callers are read from (a fixture tree), the system's callers file, and how the agents'
/// sandboxes are bounded. The packaged program passes the defaults ([`run`]): the system's `/proc`
/// and `/etc/quire`, and a scope of the user manager.
#[derive(Debug, Default)]
pub struct HarnessSeams {
    /// A fixture `/proc`, else the system's.
    pub proc_root: Option<PathBuf>,
    /// The system's callers file, else `/etc/quire/acp-callers.toml`.
    pub system_callers: Option<PathBuf>,
    /// How an agent's sandbox is bounded.
    pub limits: SandboxLimits,
}

/// Serves `AgentHost1` under the host's name until it has been quiet for [`IDLE`].
pub async fn run() -> Result<(), AgentFault> {
    run_with(HarnessSeams::default()).await
}

/// [`run`] with the seams the acceptance harness's main chooses.
pub async fn run_with(seams: HarnessSeams) -> Result<(), AgentFault> {
    let bus_fault = |e: zbus::Error| AgentFault::Bus(e.to_string());
    let bus = docket_dbus::session_connection(&env)
        .await
        .map_err(bus_fault)?;
    let HarnessSeams {
        proc_root,
        system_callers,
        limits,
    } = seams;
    let table = match &system_callers {
        Some(system) => crate::callers::read_at(system, &env),
        None => crate::callers::read(&env),
    }
    .map_err(|e| AgentFault::Callers(e.to_string()))?;
    let callers = Arc::new(Identity::on(bus.clone(), table, proc_root.as_deref()));
    let service = Arc::new(Service::new(bus.clone(), limits));
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
