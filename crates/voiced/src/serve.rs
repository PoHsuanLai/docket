//! Serving the bus.

use crate::bus::VoiceSkeleton;
use crate::command::Handle;
use crate::config::VoicedConfig;
use crate::device::AudioDevice;
use crate::engine::Core;
use crate::peer::Peers;
use crate::playback::Player;
use crate::talk::Say;
use crate::usage::{FileUse, UseSource};
use crate::warm::{BusWarm, Warm};
use crate::watch::{Deadline, Leaving};
use futures_util::StreamExt;
use porter_client::{DbusTransport, Transport};
use porter_core::Tier;
use porter_infer::Readiness;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use voice_dbus::{VOICE_BUS, VOICE_PATH};
use zbus::fdo::RequestNameFlags;

/// Everything outside the bus that voiced leans on, as seams: the audio device, inferd, the
/// engine warmer, the person's consent, and the `/proc` the callers' cgroups are read from.
#[derive(Debug)]
pub struct Seams<D, T, W, U> {
    /// PipeWire in the shipped build.
    pub device: D,
    /// inferd.
    pub transport: T,
    /// `Prepare`.
    pub warm: W,
    /// Whether voice may be used.
    pub usage: U,
    /// The `/proc` tree callers' cgroups are read from.
    pub proc_root: PathBuf,
}

/// A serving daemon: the loop runs until the bus closes.
#[derive(Debug)]
pub struct Running {
    connection: zbus::Connection,
    task: tokio::task::JoinHandle<()>,
}

impl Running {
    /// The daemon's connection.
    pub fn connection(&self) -> &zbus::Connection {
        &self.connection
    }

    /// Serves until the bus closes. A loop that panics or ends first is an error: the name would
    /// otherwise stay owned by a daemon that answers every call with `Gone`.
    pub async fn wait(self) -> Result<(), ServeError> {
        let mut messages = zbus::MessageStream::from(&self.connection);
        let closed = async { while messages.next().await.is_some() {} };
        supervise(self.task, closed).await
    }

    /// Stops the loop now.
    pub fn stop(self) {
        self.task.abort();
    }
}

/// Why serving stopped other than the bus closing.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    /// The bus failed.
    #[error("bus: {0}")]
    Bus(#[from] zbus::Error),
    /// The daemon's loop panicked.
    #[error("the voice loop panicked")]
    LoopPanicked,
    /// The daemon's loop ended while the bus was open.
    #[error("the voice loop ended")]
    LoopEnded,
}

/// Waits for `closed` (the bus going away) while watching the loop's `task`: the task ending
/// first, by panic or not, is the error; `closed` first stops the task and is `Ok`.
pub async fn supervise(
    mut task: tokio::task::JoinHandle<()>,
    closed: impl std::future::Future<Output = ()>,
) -> Result<(), ServeError> {
    let abort = task.abort_handle();
    tokio::select! {
        () = closed => {
            abort.abort();
            Ok(())
        }
        ended = &mut task => match ended {
            Err(error) if error.is_panic() => Err(ServeError::LoopPanicked),
            Ok(()) | Err(_) => Err(ServeError::LoopEnded),
        },
    }
}

/// Registers `org.quire.Voice1` on `connection` over `seams` and starts the loop. Returns when
/// the name is owned, so a caller may use the interface at once; a second daemon on the same bus
/// is an error.
pub async fn start<D, T, W, U>(
    connection: zbus::Connection,
    config: VoicedConfig,
    seams: Seams<D, T, W, U>,
) -> Result<Running, zbus::Error>
where
    D: AudioDevice + 'static,
    T: Transport + 'static,
    T::Session: 'static,
    W: Warm,
    U: UseSource,
{
    let earcons = config.earcons;
    let input = config.input.clone();
    let max_hold = std::time::Duration::from_millis(config.max_hold_ms.max(1));
    let peers = Peers::new(connection.clone(), Arc::new(config), seams.proc_root);
    let (tx, inbox) = mpsc::channel(32);
    let handle = Handle::new(tx, peers.clone());
    let device = Arc::new(seams.device);
    let (played_tx, played) = mpsc::unbounded_channel();
    let core = Core {
        conn: connection.clone(),
        handle: handle.clone(),
        peers,
        device: device.clone(),
        transport: Arc::new(seams.transport),
        warm: Arc::new(seams.warm),
        usage: seams.usage,
        earcons,
        input,
        max_hold,
        deadline: Deadline::default(),
        leaving: Leaving::default(),
        counter: 0,
        utt: None,
        node: None,
        capture: None,
        stt: None,
        cur: Vec::new(),
        say: Say::new(),
        player: Player::spawn(device, played_tx),
        played,
        stt_ready: Arc::new(Mutex::new(Readiness::Loadable)),
        tts_ready: Readiness::Loadable,
    };
    connection
        .object_server()
        .at(VOICE_PATH, VoiceSkeleton::serving(handle))
        .await?;
    let reply = connection
        .request_name_with_flags(VOICE_BUS, RequestNameFlags::DoNotQueue.into())
        .await?;
    if !matches!(
        reply,
        zbus::fdo::RequestNameReply::PrimaryOwner | zbus::fdo::RequestNameReply::AlreadyOwner
    ) {
        return Err(zbus::Error::NameTaken);
    }
    Ok(Running {
        connection,
        task: tokio::spawn(core.run(inbox)),
    })
}

/// Serves `org.quire.Voice1` on the session bus over a device: registers the three
/// interfaces, derives each caller's role from the connection, runs `voice-loop` and talks to
/// inferd through porter-client. `usage` is where the person's voice settings are read. Returns
/// when the bus closes, or with an error if the loop dies first.
pub async fn serve<D: AudioDevice + 'static>(
    config: VoicedConfig,
    device: D,
    usage: FileUse,
) -> Result<(), ServeError> {
    let connection = zbus::Connection::session().await?;
    let seams = Seams {
        device,
        transport: DbusTransport::over(connection.clone()),
        warm: BusWarm::new(connection.clone(), Tier::Balanced),
        usage,
        proc_root: PathBuf::from("/proc"),
    };
    start(connection, config, seams).await?.wait().await
}
