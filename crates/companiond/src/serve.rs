//! Serving `org.quire.Companion1`: the root object (`Prepare`, `Open`, `Ask`, `Close`, `Roster`,
//! `Front`) and one answer object per task (`/org/quire/Companion1/answer/<task>`). The roster,
//! the front task and the answers are read from what the loop last wrote (`Shared`), never from
//! the companion itself, so `Roster()` answers while a planner turn waits on a confirmation.
//! Members carry no doc comments: zbus copies them into the introspection, which is held to
//! `dbus/org.quire.Companion1.xml`.

use crate::Companiond;
use crate::Shared;
use crate::helpers_bus::{Helpers2Object, HelpersObject};
use crate::routines::RoutineDesk;
use crate::speaker::{Call, Speaker, Speakers};
use companion_wire::AskWire;
use docket_client::Transport as IntentsTransport;
use docket_core::{SessionOpen, StartedFrom, UserTurn};
use docket_dbus::callers::Identity;
use docket_dbus::{
    BusConnection, COMPANION_BUS, COMPANION_PATH, Details, INTENTS_BUS, MessageProxy, answer_path,
};
use docket_tasks::{Change, ServeFault};
use futures_util::lock::Mutex;
use porter_client::Transport as InferTransport;
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use zbus::fdo;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

/// How often time is told to the companion: side conversations end and the idle pass starts on
/// this beat.
const TICK: Duration = Duration::from_secs(5);

fn failed(error: ServeFault) -> fdo::Error {
    match error {
        ServeFault::UnknownSession | ServeFault::NoSuchCard => {
            fdo::Error::InvalidArgs(error.to_string())
        }
        other => fdo::Error::Failed(other.to_string()),
    }
}

pub(crate) fn bad(error: serde_json::Error) -> fdo::Error {
    fdo::Error::InvalidArgs(error.to_string())
}

pub(crate) fn json<T: serde::Serialize>(value: &T) -> fdo::Result<String> {
    serde_json::to_string(value).map_err(|e| fdo::Error::Failed(e.to_string()))
}

fn path_of(task: &TaskId) -> fdo::Result<OwnedObjectPath> {
    OwnedObjectPath::try_from(answer_path(task)).map_err(|e| fdo::Error::Failed(e.to_string()))
}

struct Root<P: InferTransport, I: IntentsTransport> {
    companion: Arc<Mutex<Companiond<P, I>>>,
    shared: Arc<Shared>,
    speakers: Speakers,
}

impl<P: InferTransport, I: IntentsTransport> Root<P, I> {
    /// The shell speaks for the person, and so does a terminal for the conversation (`Open`,
    /// `Ask`, `Close`): an ask carries the turn the router recorded, and a turn it never recorded
    /// must not be taken from anybody else. `Told` and `Act` are the shell's alone.
    async fn require(&self, header: &Header<'_>, call: Call) -> fdo::Result<Speaker> {
        self.speakers.require(header, call).await
    }
}

#[zbus::interface(name = "org.quire.Companion1")]
impl<P: InferTransport + 'static, I: IntentsTransport + 'static> Root<P, I> {
    async fn prepare(&self, options: Details) -> fdo::Result<()> {
        let _ = options;
        Ok(())
    }

    async fn open(&self, open: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        let speaker = self.require(&header, Call::Open).await?;
        let open: SessionOpen = serde_json::from_str(&open).map_err(bad)?;
        // Only what the daemon itself saw of the caller's cgroup counts, never what the caller wrote.
        let open = SessionOpen {
            started_from: match speaker {
                Speaker::Terminal(scope) => Some(StartedFrom::Terminal(scope)),
                Speaker::Shell | Speaker::Editor => None,
            },
            ..open
        };
        let opened = self
            .companion
            .lock()
            .await
            .open(open)
            .await
            .map_err(failed)?;
        json(&opened)
    }

    async fn ask(
        &self,
        ask: String,
        options: Details,
        #[zbus(header)] header: Header<'_>,
        #[zbus(object_server)] server: &zbus::ObjectServer,
    ) -> fdo::Result<OwnedObjectPath> {
        let _ = options;
        self.require(&header, Call::Ask).await?;
        let ask: AskWire = serde_json::from_str(&ask).map_err(bad)?;
        // Whatever background work holds the model yields before the loop starts.
        self.shared.interrupt();
        let begun = self.companion.lock().await.begin_ask(ask).map_err(failed)?;
        let path = path_of(&begun.task)?;
        // The answer object exists before the caller has its path.
        server
            .at(
                path.clone(),
                AnswerObject {
                    companion: self.companion.clone(),
                    shared: self.shared.clone(),
                    task: begun.task.clone(),
                    speakers: self.speakers.clone(),
                },
            )
            .await?;
        let companion = self.companion.clone();
        tokio::spawn(async move {
            let _ = companion.lock().await.run_begun(begun).await;
        });
        Ok(path)
    }

    async fn close(&self, session: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.require(&header, Call::Close).await?;
        let session =
            SessionId::parse(&session).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        // A turn that is still waiting on the model or the reader ends now, so the lock is free
        // for the close instead of held until the peer answers.
        if let Some(task) = self.shared.task_of(&session) {
            self.shared.cancel(&task);
        }
        self.companion
            .lock()
            .await
            .close(session)
            .await
            .map_err(failed)
    }

    async fn told(
        &self,
        agent: String,
        space: String,
        turn: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<()> {
        self.require(&header, Call::Told).await?;
        let agent: AgentRef = serde_json::from_str(&agent).map_err(bad)?;
        let space = SpaceId::parse(&space).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
        let turn: UserTurn = serde_json::from_str(&turn).map_err(bad)?;
        self.companion.lock().await.told(agent, space, turn);
        Ok(())
    }

    async fn roster(&self) -> fdo::Result<String> {
        json(&self.shared.roster())
    }

    async fn front(&self) -> fdo::Result<String> {
        json(&self.shared.front())
    }

    #[zbus(signal)]
    async fn answer_added(emitter: &SignalEmitter<'_>, answer: ObjectPath<'_>) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn answer_removed(
        emitter: &SignalEmitter<'_>,
        answer: ObjectPath<'_>,
    ) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn roster_changed(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

struct AnswerObject<P: InferTransport, I: IntentsTransport> {
    companion: Arc<Mutex<Companiond<P, I>>>,
    shared: Arc<Shared>,
    task: TaskId,
    speakers: Speakers,
}

impl<P: InferTransport, I: IntentsTransport> AnswerObject<P, I> {
    /// An answer is the person's words and what the router showed of them: only the shell or a
    /// terminal reads it.
    async fn readable(&self, header: Option<&Header<'_>>) -> fdo::Result<()> {
        self.speakers.require_read(header).await.map(|_| ())
    }
}

#[zbus::interface(name = "org.quire.Companion1.Answer")]
impl<P: InferTransport + 'static, I: IntentsTransport + 'static> AnswerObject<P, I> {
    async fn act(
        &self,
        action: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        self.speakers.require(&header, Call::Act).await?;
        let card = docket_tasks::card_id(&action).map_err(failed)?;
        let acting = self
            .companion
            .lock()
            .await
            .begin_act(&self.task, &card)
            .await
            .map_err(failed)?;
        let path = OwnedObjectPath::try_from(
            acting
                .request()
                .map_or_else(|| docket_dbus::request_path(acting.id()), str::to_owned),
        )
        .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        tokio::spawn(crate::follow::follow(self.companion.clone(), acting));
        Ok(path)
    }

    async fn cancel(&self, #[zbus(header)] header: Header<'_>) -> fdo::Result<()> {
        self.speakers.require(&header, Call::Cancel).await?;
        self.shared.cancel(&self.task);
        Ok(())
    }

    #[zbus(signal)]
    async fn updated(emitter: &SignalEmitter<'_>, view: &str) -> zbus::Result<()>;

    #[zbus(property)]
    async fn routing(&self, #[zbus(header)] header: Option<Header<'_>>) -> fdo::Result<String> {
        self.readable(header.as_ref()).await?;
        let notes = self
            .shared
            .route_of(&self.task)
            .ok_or_else(|| fdo::Error::Failed("no such answer".into()))?;
        serde_json::to_string(&notes).map_err(|e| fdo::Error::Failed(e.to_string()))
    }

    #[zbus(property)]
    async fn session(&self, #[zbus(header)] header: Option<Header<'_>>) -> fdo::Result<String> {
        self.readable(header.as_ref()).await?;
        match self.shared.session_of(&self.task) {
            Some(session) => Ok(session.as_str().to_owned()),
            None => Err(fdo::Error::Failed("no such answer".into())),
        }
    }

    #[zbus(property)]
    async fn view(&self, #[zbus(header)] header: Option<Header<'_>>) -> fdo::Result<String> {
        self.readable(header.as_ref()).await?;
        match self.shared.answer(&self.task) {
            Some(answer) => json(&answer),
            None => Err(fdo::Error::Failed("no such answer".into())),
        }
    }
}

fn bus(error: zbus::Error) -> ServeFault {
    ServeFault::Bus(error.to_string())
}

/// Forwards what the loop wrote to the bus: answer objects appear, change and go away, and the
/// roster changes, each as a content-free signal beside the object.
async fn forward<P, I>(
    connection: BusConnection,
    shared: Arc<Shared>,
    companion: Arc<Mutex<Companiond<P, I>>>,
    speakers: Speakers,
) where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let mut changes = shared.subscribe();
    let server = connection.object_server().clone();
    while let Ok(change) = changes.recv().await {
        match change {
            Change::AnswerAdded(task) => {
                let path = answer_path(&task);
                let object = AnswerObject {
                    companion: companion.clone(),
                    shared: shared.clone(),
                    task,
                    speakers: speakers.clone(),
                };
                let _ = server.at(path.as_str(), object).await;
                if let Ok(object) = ObjectPath::try_from(path.as_str()) {
                    let _ = connection
                        .emit_signal(
                            None::<&str>,
                            COMPANION_PATH,
                            "org.quire.Companion1",
                            "AnswerAdded",
                            &(object,),
                        )
                        .await;
                }
            }
            Change::AnswerChanged(task) => {
                let path = answer_path(&task);
                if let Some(view) = shared
                    .answer(&task)
                    .and_then(|a| serde_json::to_string(&a).ok())
                {
                    let _ = connection
                        .emit_signal(
                            None::<&str>,
                            path.as_str(),
                            "org.quire.Companion1.Answer",
                            "Updated",
                            &(view,),
                        )
                        .await;
                }
            }
            Change::AnswerRemoved(task) => {
                let path = answer_path(&task);
                let _ = server.remove::<AnswerObject<P, I>, _>(path.as_str()).await;
                if let Ok(object) = ObjectPath::try_from(path.as_str()) {
                    let _ = connection
                        .emit_signal(
                            None::<&str>,
                            COMPANION_PATH,
                            "org.quire.Companion1",
                            "AnswerRemoved",
                            &(object,),
                        )
                        .await;
                }
            }
            Change::RosterChanged => {
                let _ = connection
                    .emit_signal(
                        None::<&str>,
                        COMPANION_PATH,
                        "org.quire.Companion1",
                        "RosterChanged",
                        &(),
                    )
                    .await;
            }
            Change::HelpersChanged => {
                for interface in [
                    "org.quire.Companion1.Helpers",
                    "org.quire.Companion1.Helpers2",
                ] {
                    let _ = connection
                        .emit_signal(None::<&str>, COMPANION_PATH, interface, "Changed", &())
                        .await;
                }
            }
        }
    }
}

/// Claims `org.quire.Companion1` on `connection`, serves the root object and one answer object
/// per task, listens for `Message.Arrived` and keeps time. The callers are told apart by `who`
/// (the shell is the process it names as the shell's unit; a terminal's child is read from the
/// system's `/proc`). Returns once the name is ours; the connection keeps serving until it closes.
pub async fn serve_on<P, I>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I>>>,
    who: Arc<Identity>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let proc_root = PathBuf::from("/proc");
    serve_on_rooted(connection, companion, who, proc_root).await
}

/// [`serve_on`] reading terminals' cgroups under `proc_root` instead of `/proc`: how a test
/// presents its processes as a terminal. The daemon's own `serve_on` always reads `/proc`.
pub async fn serve_on_rooted<P, I>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I>>>,
    who: Arc<Identity>,
    proc_root: PathBuf,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    serve_with_routines(connection, companion, who, proc_root, None).await
}

/// [`serve_on_rooted`] with the routines the daemon keeps, which `Companion1.Helpers2` lists and
/// changes. With none, its routine calls say routines are not kept here.
pub async fn serve_with_routines<P, I>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I>>>,
    who: Arc<Identity>,
    proc_root: PathBuf,
    routines: Option<Arc<dyn RoutineDesk>>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let (shared, shell, desk) = {
        let held = companion.lock().await;
        (held.shared.clone(), held.shell.clone(), held.desk())
    };
    let speakers = Speakers::new(connection.clone(), who, shell, proc_root);
    connection
        .object_server()
        .at(
            COMPANION_PATH,
            Root {
                companion: companion.clone(),
                shared: shared.clone(),
                speakers: speakers.clone(),
            },
        )
        .await
        .map_err(bus)?;
    // The helpers are served beside the conversation when the daemon keeps any.
    if let Some(desk) = desk {
        connection
            .object_server()
            .at(
                COMPANION_PATH,
                HelpersObject {
                    desk: desk.clone(),
                    speakers: speakers.clone(),
                },
            )
            .await
            .map_err(bus)?;
        connection
            .object_server()
            .at(
                COMPANION_PATH,
                Helpers2Object {
                    desk,
                    routines,
                    speakers: speakers.clone(),
                },
            )
            .await
            .map_err(bus)?;
    }
    let reply = connection
        .request_name_with_flags(COMPANION_BUS, RequestNameFlags::DoNotQueue.into())
        .await
        .map_err(bus)?;
    match reply {
        RequestNameReply::PrimaryOwner | RequestNameReply::AlreadyOwner => {}
        RequestNameReply::InQueue | RequestNameReply::Exists => {
            return Err(ServeFault::Bus(format!("{COMPANION_BUS} is taken")));
        }
    }
    tokio::spawn(forward(
        connection.clone(),
        shared,
        companion.clone(),
        speakers,
    ));
    tokio::spawn(arrivals(connection.clone(), companion.clone()));
    tokio::spawn(ticking(companion));
    Ok(())
}

/// A message arrived for an agent: read the inbox of the agents the companion runs.
async fn arrivals<P, I>(connection: BusConnection, companion: Arc<Mutex<Companiond<P, I>>>)
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    use futures_util::StreamExt;
    let Ok(proxy) = MessageProxy::builder(&connection)
        .destination(INTENTS_BUS)
        .and_then(|b| b.path(docket_dbus::INTENTS_PATH))
    else {
        return;
    };
    let Ok(proxy) = proxy.build().await else {
        return;
    };
    let Ok(mut signals) = proxy.receive_arrived().await else {
        return;
    };
    while signals.next().await.is_some() {
        let _ = companion.lock().await.arrived().await;
    }
}

async fn ticking<P, I>(companion: Arc<Mutex<Companiond<P, I>>>)
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let mut beat = tokio::time::interval(TICK);
    loop {
        beat.tick().await;
        // A turn in flight holds the companion: time is told again on the next beat, never
        // queued behind it.
        if let Some(mut held) = companion.try_lock() {
            let _ = held.tick().await;
        }
    }
}

/// Claims `org.quire.Companion1` on the session bus and serves it until the connection closes,
/// telling callers apart by `companiond-callers.toml` (the system's under `/etc/quire`, the
/// person's over it).
pub async fn serve<P, I>(companion: Arc<Mutex<Companiond<P, I>>>) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    use futures_util::StreamExt;
    let connection = BusConnection::session().await.map_err(bus)?;
    let table = crate::callers::read(&|key: &str| std::env::var(key).ok())
        .map_err(|e| ServeFault::Config(e.to_string()))?;
    let who = Arc::new(Identity::on_system(connection.clone(), table));
    serve_on(&connection, companion, who).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
