//! Serving `org.quire.Companion1`: the root object (`Prepare`, `Open`, `Ask`, `Close`, `Roster`,
//! `Front`) and one answer object per task (`/org/quire/Companion1/answer/<task>`). The roster,
//! the front task and the answers are read from what the loop last wrote (`Shared`), never from
//! the companion itself, so `Roster()` answers while a planner turn waits on a confirmation.
//! Members carry no doc comments: zbus copies them into the introspection, which is held to
//! `dbus/org.quire.Companion1.xml`.

use crate::fault::ServeFault;
use crate::runtime::Companiond;
use crate::shared::{Change, Shared};
use companion_wire::AskWire;
use docket_client::Transport as IntentsTransport;
use docket_core::{SessionOpen, UserTurn};
use docket_dbus::{
    BusConnection, COMPANION_BUS, COMPANION_PATH, Details, INTENTS_BUS, MessageProxy, answer_path,
};
use porter_client::Transport as InferTransport;
use porter_core::AppName;
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;
use zbus::fdo;
use zbus::fdo::DBusProxy;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::names::BusName;
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

fn bad(error: serde_json::Error) -> fdo::Error {
    fdo::Error::InvalidArgs(error.to_string())
}

fn json<T: serde::Serialize>(value: &T) -> fdo::Result<String> {
    serde_json::to_string(value).map_err(|e| fdo::Error::Failed(e.to_string()))
}

fn path_of(task: &TaskId) -> fdo::Result<OwnedObjectPath> {
    OwnedObjectPath::try_from(answer_path(task)).map_err(|e| fdo::Error::Failed(e.to_string()))
}

struct Root<P: InferTransport, I: IntentsTransport> {
    companion: Arc<Mutex<Companiond<P, I>>>,
    shared: Arc<Shared>,
    shell: AppName,
    connection: BusConnection,
}

/// Only the shell speaks for the person: an ask carries the turn it recorded, and a turn the
/// router never recorded must not be taken from anybody else; `Open` and `Close` make and end the
/// person's conversations, and `Act` presses a card for them, so they are the shell's too.
async fn require_shell(
    connection: &BusConnection,
    shell: &AppName,
    header: &Header<'_>,
) -> fdo::Result<()> {
    let sender = header.sender().map(ToString::to_string).unwrap_or_default();
    let proxy = DBusProxy::new(connection).await?;
    let name = BusName::try_from(shell.as_str()).map_err(|e| fdo::Error::Failed(e.to_string()))?;
    match proxy.get_name_owner(name).await {
        Ok(owner) if owner.as_str() == sender => Ok(()),
        _ => Err(fdo::Error::AccessDenied(
            "only the shell speaks for the person".into(),
        )),
    }
}

impl<P: InferTransport, I: IntentsTransport> Root<P, I> {
    async fn require_shell(&self, header: &Header<'_>) -> fdo::Result<()> {
        require_shell(&self.connection, &self.shell, header).await
    }
}

#[zbus::interface(name = "org.quire.Companion1")]
impl<P: InferTransport + 'static, I: IntentsTransport + 'static> Root<P, I> {
    async fn prepare(&self, options: Details) -> fdo::Result<()> {
        let _ = options;
        Ok(())
    }

    async fn open(&self, open: String, #[zbus(header)] header: Header<'_>) -> fdo::Result<String> {
        self.require_shell(&header).await?;
        let open: SessionOpen = serde_json::from_str(&open).map_err(bad)?;
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
        self.require_shell(&header).await?;
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
                    shell: self.shell.clone(),
                    connection: self.connection.clone(),
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
        self.require_shell(&header).await?;
        let session =
            SessionId::parse(&session).map_err(|e| fdo::Error::InvalidArgs(e.to_string()))?;
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
        self.require_shell(&header).await?;
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
    shell: AppName,
    connection: BusConnection,
}

#[zbus::interface(name = "org.quire.Companion1.Answer")]
impl<P: InferTransport + 'static, I: IntentsTransport + 'static> AnswerObject<P, I> {
    async fn act(
        &self,
        action: String,
        #[zbus(header)] header: Header<'_>,
    ) -> fdo::Result<OwnedObjectPath> {
        require_shell(&self.connection, &self.shell, &header).await?;
        let card = crate::act::card_id(&action).map_err(failed)?;
        let acting = self
            .companion
            .lock()
            .await
            .begin_act(&self.task, &card)
            .await
            .map_err(failed)?;
        let path = OwnedObjectPath::try_from(acting.request_path())
            .map_err(|e| fdo::Error::Failed(e.to_string()))?;
        tokio::spawn(crate::act::follow(self.companion.clone(), acting));
        Ok(path)
    }

    async fn cancel(&self) -> fdo::Result<()> {
        self.shared.cancel(&self.task);
        Ok(())
    }

    #[zbus(signal)]
    async fn updated(emitter: &SignalEmitter<'_>, view: &str) -> zbus::Result<()>;

    #[zbus(property)]
    async fn view(&self) -> fdo::Result<String> {
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
    shell: AppName,
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
                    shell: shell.clone(),
                    connection: connection.clone(),
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
        }
    }
}

/// Claims `org.quire.Companion1` on `connection`, serves the root object and one answer object
/// per task, listens for `Message.Arrived` and keeps time. Returns once the name is ours; the
/// connection keeps serving until it closes.
pub async fn serve_on<P, I>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I>>>,
) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    let (shared, shell) = {
        let held = companion.lock().await;
        (held.shared.clone(), held.shell.clone())
    };
    connection
        .object_server()
        .at(
            COMPANION_PATH,
            Root {
                companion: companion.clone(),
                shared: shared.clone(),
                shell: shell.clone(),
                connection: connection.clone(),
            },
        )
        .await
        .map_err(bus)?;
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
        shell,
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
        let _ = companion.lock().await.tick().await;
    }
}

/// Claims `org.quire.Companion1` on the session bus and serves it until the connection closes.
pub async fn serve<P, I>(companion: Arc<Mutex<Companiond<P, I>>>) -> Result<(), ServeFault>
where
    P: InferTransport + 'static,
    I: IntentsTransport + 'static,
{
    use futures_util::StreamExt;
    let connection = BusConnection::session().await.map_err(bus)?;
    serve_on(&connection, companion).await?;
    let mut messages = zbus::MessageStream::from(&connection);
    while messages.next().await.is_some() {}
    Ok(())
}
