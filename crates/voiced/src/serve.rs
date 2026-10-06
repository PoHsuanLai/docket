//! Serving the bus.

use crate::bus::VoiceSkeleton;
use crate::command::Handle;
use crate::config::VoicedConfig;
use crate::device::AudioDevice;
use crate::engine::Core;
use crate::names::{VOICE_BUS, VOICE_PATH};
use crate::peer::Peers;
use crate::playback::Player;
use crate::talk::Say;
use crate::usage::{FileUse, UseSource};
use crate::warm::{BusWarm, Warm};
use futures_util::StreamExt;
use porter_client::{DbusTransport, Transport};
use porter_core::Tier;
use porter_infer::Readiness;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
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

    /// Serves until the bus closes.
    pub async fn wait(self) {
        let mut messages = zbus::MessageStream::from(&self.connection);
        while messages.next().await.is_some() {}
        self.task.abort();
    }

    /// Stops the loop now.
    pub fn stop(self) {
        self.task.abort();
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
/// inferd through porter-client. Returns when the bus closes.
pub async fn serve<D: AudioDevice + 'static>(
    config: VoicedConfig,
    device: D,
) -> Result<(), zbus::Error> {
    let connection = zbus::Connection::session().await?;
    let seams = Seams {
        device,
        transport: DbusTransport::over(connection.clone()),
        warm: BusWarm::new(connection.clone(), Tier::Balanced),
        usage: FileUse::sill_default(),
        proc_root: PathBuf::from("/proc"),
    };
    start(connection, config, seams).await?.wait().await;
    Ok(())
}
