//! The audio device seam, pure. Capture reads only a physical source node, never a sink monitor,
//! so a daemon built on it cannot transcribe other apps' audio. The traits here are what the
//! voice daemon plays against; `voice-pipewire` is the shipped backend and [`FakeAudioDevice`]
//! (feature `testing`) the scripted one.

mod choose;
mod device;
#[cfg(feature = "testing")]
mod fake;
mod node;
mod pcm;
mod snapshot;

pub use choose::{Chosen, ChosenBy, choose_capture};
pub use device::{
    AudioDevice, CaptureFormat, CaptureStream, DeviceError, PlaybackFormat, PlaybackStream,
    Snapshot,
};
#[cfg(feature = "testing")]
pub use fake::{FakeAudioDevice, FakeCapture, FakePlayback};
pub use node::{AudioNode, MediaClass, NodeId, NodeKind};
pub use pcm::samples_of;
pub use snapshot::{SNAPSHOT_BUDGET, snapshot_before};
