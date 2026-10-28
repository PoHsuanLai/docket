//! The PipeWire backend of the `voice-audio` device seam (spike V-A, FINDINGS.md). Capture asks
//! PipeWire's adapter for 16 kHz mono S16 from one physical `Audio/Source` node by name; a
//! monitor or a sink is refused before anything connects, so a daemon built on it cannot hear
//! other apps. Each stream runs a main loop on its own thread (PipeWire objects are not `Send`);
//! frames cross to the async side over a channel and the stream is closed when its handle drops.

mod capture;
mod default_source;
mod device;
mod format;
mod nodes;
mod playback;

pub use capture::PipeWireCapture;
pub use default_source::{CONFIGURED_KEY, DefaultSources, RESOLVED_KEY, name_in};
pub use device::PipeWireDevice;
pub use playback::PipeWirePlayback;
