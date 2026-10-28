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

mod object;
mod watchdog;

use super::calls::{Calls, NoSuchTurn};
use super::fault::AgentFault;
use super::folders::folder;
use super::hand_words::{end_of, refusal_of_fault, refusal_of_open};
use super::helper_host::HelperHost;
use super::host::{Hosted, open_on};
use super::live::Live;
use super::places::Places;
use super::wire::{self, SandboxLimits, Tools};
use bulkhead::NetworkMode;
use companion_model::helper::{AppsAsk, AppsReply};
use docket_acp_host::Fallback;
use docket_core::{HandOver, HandRefusal, HandReply, Handed, Reveal, UserTurn};
use docket_dbus::callers::Identity;
use docket_dbus::{AGENT_HOST_BUS, AGENT_HOST_PATH, BusConnection};
use docket_launch::dbus::DbusAccounts;
use docket_permissions_core::ProgramName;
use docket_session::{BackendEvent, EndCause, SessionHost};
use porter_daemon::ProcRoot;
use prov::SpaceId;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::Instant;
use tokio::sync::{Mutex, OnceCell};

/// One agent's open session.
struct Active {
    program: ProgramName,
    label: String,
    hosted: Hosted<DbusAccounts>,
    /// It stopped answering: its session is closed, and the next turn opens a new one.
    broken: bool,
}

/// The service: who may call it, the pseudo-app every use shares, the assistant's agent session,
/// the helpers' agents, and when it was last used.
pub struct Service {
    bus: BusConnection,
    limits: SandboxLimits,
    places: Places,
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
    /// A service with nothing started, on `bus`, whose agents' sandboxes are bounded by `limits`
    /// and whose places come from `places`.
    pub fn new(bus: BusConnection, limits: SandboxLimits, places: Places) -> Self {
        Self {
            bus,
            limits,
            places,
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
                    wire::allowed(&self.places)?,
                    Tools::Wanted,
                    Fallback::Off,
                    self.bus.clone(),
                    self.limits,
                    self.places.clone(),
                )
                .await?;
                let live = Live::serve(wiring, NetworkMode::Host)
                    .await
                    .map_err(|why| AgentFault::Bus(why.to_string()))?;
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
        if active.broken {
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
        let cwd = folder(program, live.places()).map_err(|why| HandRefusal::Failed {
            message: why.to_string(),
        })?;
        let hosted = open_on(live, program.clone(), &cwd, space)
            .await
            .map_err(|error| refusal_of_open(error, &label))?;
        Ok(Active {
            program: program.clone(),
            label,
            hosted,
            broken: false,
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

    /// Closes the session still open and ends the helpers' agents, on the way out.
    pub async fn close(&self) {
        Self::finish(&mut *self.assistant.lock().await).await;
        if let Some(helpers) = self.helpers.get() {
            helpers.shutdown().await;
        }
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
            self.broken = true;
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
                    self.broken = true;
                    return Err(broke(&label));
                }
            }
        }
    }
}

/// What only the acceptance harness chooses, as docket-acp's `HarnessSeams` does: the `/proc` the
/// callers are read from (a fixture tree), the system's callers file, and how the agents'
/// sandboxes are bounded. The packaged program passes the defaults ([`run`]): the system's `/proc`
/// and `/etc/quire`, and a scope of the user manager.
#[derive(Debug)]
pub struct HarnessSeams {
    /// Where callers' processes are read: the system's `/proc`, or a fixture tree.
    pub proc_root: ProcRoot,
    /// The system's callers file, else `/etc/quire/acp-callers.toml`.
    pub system_callers: Option<PathBuf>,
    /// How an agent's sandbox is bounded.
    pub limits: SandboxLimits,
}

impl Default for HarnessSeams {
    fn default() -> Self {
        Self {
            proc_root: ProcRoot::System,
            system_callers: None,
            limits: SandboxLimits::default(),
        }
    }
}

/// Serves `AgentHost1` under the host's name until it has been quiet for ten minutes. `env` is
/// the environment `main` hands in.
pub async fn run(
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> Result<(), AgentFault> {
    run_with(HarnessSeams::default(), env).await
}

/// [`run`] with the seams the acceptance harness's main chooses.
pub async fn run_with(
    seams: HarnessSeams,
    env: impl Fn(&str) -> Option<String> + Send + Sync + 'static,
) -> Result<(), AgentFault> {
    let bus_fault = |e: zbus::Error| AgentFault::Bus(e.to_string());
    let places = Places::read(env);
    let var = |name: &str| places.var(name);
    let bus = docket_dbus::session_connection(&var)
        .await
        .map_err(bus_fault)?;
    let HarnessSeams {
        proc_root,
        system_callers,
        limits,
    } = seams;
    let table = match &system_callers {
        Some(system) => crate::callers::read_at(system, &var),
        None => crate::callers::read(&var),
    }
    .map_err(|e| AgentFault::Callers(e.to_string()))?;
    let callers = Arc::new(Identity::on(bus.clone(), table, proc_root.fixture()));
    let service = Arc::new(Service::new(bus.clone(), limits, places.clone()));
    // Served before the name is claimed, so the first call finds it.
    bus.object_server()
        .at(
            AGENT_HOST_PATH,
            object::Object {
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
