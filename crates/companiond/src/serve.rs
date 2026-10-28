//! Serving `org.quire.Companion1`: the root object (`Prepare`, `Open`, `Ask`, `Close`, `Roster`,
//! `Front`) and one answer object per task (`/org/quire/Companion1/answer/<task>`). The roster,
//! the front task and the answers are read from what the loop last wrote (`Shared`), never from
//! the companion itself, so `Roster()` answers while a planner turn waits on a confirmation.
//! Members carry no doc comments: zbus copies them into the introspection, which is held to
//! `dbus/org.quire.Companion1.xml`.

use crate::Companiond;
use crate::Shared;
use crate::helpers_bus::{Helpers2Object, HelpersObject, TerminalsObject};
use crate::speaker::Speakers;
use companion_core::{Call, Speaker};
use companion_wire::AskWire;
use docket_client::Transport as IntentsTransport;
use docket_core::UserTurn;
use docket_dbus::callers::Identity;
use docket_dbus::{BusConnection, COMPANION_BUS, COMPANION_PATH, Details, answer_path};
use docket_helpers::RoutineDesk;
use docket_planner::Planner;
use docket_tasks::{Change, ServeFault};
use docket_vocab::Clock;
use docket_wire::{SessionOpen, StartedFrom};
use futures_util::lock::Mutex;
use porter_daemon::ProcRoot;
use prov::{AgentRef, SessionId, SpaceId, TaskId};
use std::sync::Arc;
use zbus::fdo;
use zbus::fdo::{RequestNameFlags, RequestNameReply};
use zbus::message::Header;
use zbus::object_server::SignalEmitter;
use zbus::zvariant::{ObjectPath, OwnedObjectPath};

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

struct Root<P: Planner, I: IntentsTransport, K: Clock> {
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    shared: Arc<Shared>,
    speakers: Speakers,
}

impl<P: Planner, I: IntentsTransport, K: Clock> Root<P, I, K> {
    /// The shell speaks for the person, and so does a terminal for the conversation (`Open`,
    /// `Ask`, `Close`): an ask carries the turn the router recorded, and a turn it never recorded
    /// must not be taken from anybody else. `Told` and `Act` are the shell's alone.
    async fn require(&self, header: &Header<'_>, call: Call) -> fdo::Result<Speaker> {
        self.speakers.require(header, call).await
    }
}

#[zbus::interface(name = "org.quire.Companion1")]
impl<P: Planner + 'static, I: IntentsTransport + 'static, K: Clock + 'static> Root<P, I, K> {
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

struct AnswerObject<P: Planner, I: IntentsTransport, K: Clock> {
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    shared: Arc<Shared>,
    task: TaskId,
    speakers: Speakers,
}

impl<P: Planner, I: IntentsTransport, K: Clock> AnswerObject<P, I, K> {
    /// An answer is the person's words and what the router showed of them: only the shell or a
    /// terminal reads it.
    async fn readable(&self, header: Option<&Header<'_>>) -> fdo::Result<()> {
        self.speakers.require_read(header).await.map(|_| ())
    }
}

#[zbus::interface(name = "org.quire.Companion1.Answer")]
impl<P: Planner + 'static, I: IntentsTransport + 'static, K: Clock + 'static>
    AnswerObject<P, I, K>
{
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
async fn forward<P, I, K>(
    connection: BusConnection,
    shared: Arc<Shared>,
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    speakers: Speakers,
) where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
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
                let _ = server
                    .remove::<AnswerObject<P, I, K>, _>(path.as_str())
                    .await;
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
                    "org.quire.Companion1.Terminals",
                ] {
                    let _ = connection
                        .emit_signal(None::<&str>, COMPANION_PATH, interface, "Changed", &())
                        .await;
                }
            }
        }
    }
}

/// [`serve_on_rooted`] with the routines the daemon keeps, which `Companion1.Helpers2` lists and
/// changes. With none, its routine calls say routines are not kept here.
pub async fn serve_with_routines<P, I, K>(
    connection: &BusConnection,
    companion: Arc<Mutex<Companiond<P, I, K>>>,
    who: Arc<Identity>,
    proc_root: ProcRoot,
    routines: Option<Arc<dyn RoutineDesk>>,
) -> Result<(), ServeFault>
where
    P: Planner + 'static,
    I: IntentsTransport + 'static,
    K: Clock + 'static,
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
                    desk: desk.clone(),
                    routines,
                    speakers: speakers.clone(),
                },
            )
            .await
            .map_err(bus)?;
        connection
            .object_server()
            .at(
                COMPANION_PATH,
                TerminalsObject {
                    desk,
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
    tokio::spawn(crate::background::arrivals(
        connection.clone(),
        companion.clone(),
    ));
    tokio::spawn(crate::run_signals::watch(
        connection.clone(),
        companion.clone(),
    ));
    tokio::spawn(crate::background::ticking(companion));
    Ok(())
}
