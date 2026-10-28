//! The device traits, the formats, the errors and one enumeration's result.

use crate::node::AudioNode;
use serde::{Deserialize, Serialize};
use std::future::Future;

/// The capture format: 16 kHz mono S16, converted by PipeWire's adapter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaptureFormat {
    /// Samples per second.
    pub rate: u32,
}

/// The playback format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaybackFormat {
    /// Samples per second (the TTS engine's own).
    pub rate: u32,
}

/// Why the device failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum DeviceError {
    /// No physical source.
    #[error("no microphone")]
    NoSource,
    /// The system denied it.
    #[error("denied")]
    Denied,
    /// The stream ended.
    #[error("stream closed")]
    Closed,
    /// The system did not answer in time.
    #[error("timed out")]
    TimedOut,
}

/// What one enumeration found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    /// The nodes present.
    pub sources: Vec<AudioNode>,
    /// The node name of the default source, if the system names one.
    pub default: Option<String>,
}

/// A running capture.
pub trait CaptureStream: Send + Sync {
    /// The next frame of samples, or none when the stream ended.
    fn next(&mut self) -> impl Future<Output = Option<Vec<i16>>> + Send;
}

/// A running playback.
pub trait PlaybackStream: Send {
    /// Plays samples.
    fn write(&mut self, samples: &[i16]) -> impl Future<Output = Result<(), DeviceError>> + Send;
    /// Fades out over `ms` and drains.
    fn fade_out(&mut self, ms: u32) -> impl Future<Output = ()> + Send;
}

/// The device: PipeWire in the shipped build, a fake in tests.
pub trait AudioDevice: Send + Sync {
    /// The capture stream type.
    type Capture: CaptureStream;
    /// The playback stream type.
    type Playback: PlaybackStream;

    /// The nodes now present and the default source, from one look at the system. The shipped
    /// device does this in one enumeration, off the async threads.
    fn snapshot(&self) -> impl Future<Output = Result<Snapshot, DeviceError>> + Send {
        async {
            Ok(Snapshot {
                sources: self.sources().await,
                default: self.default_source().await,
            })
        }
    }
    /// The nodes now present.
    fn sources(&self) -> impl Future<Output = Vec<AudioNode>> + Send;
    /// The node name of the person's default source, if the system names one.
    fn default_source(&self) -> impl Future<Output = Option<String>> + Send {
        async { None }
    }
    /// Opens capture on a node (the caller chose it with [`crate::choose_capture`]).
    fn open_capture(
        &self,
        node: &AudioNode,
        format: CaptureFormat,
    ) -> impl Future<Output = Result<Self::Capture, DeviceError>> + Send;
    /// Opens playback on the default output.
    fn open_playback(
        &self,
        format: PlaybackFormat,
    ) -> impl Future<Output = Result<Self::Playback, DeviceError>> + Send;
}
