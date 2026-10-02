//! Serving the bus.

use crate::config::VoicedConfig;
use crate::device::AudioDevice;

/// Serves `org.quire.Voice1` on the session bus over a device: registers the three
/// interfaces, derives each caller's role from the connection, runs `voice-loop` and talks to
/// inferd through porter-client.
pub async fn serve<D: AudioDevice>(config: VoicedConfig, device: D) -> Result<(), zbus::Error> {
    let _ = (config, device);
    todo!(
        "serve: Begin from the shell role only; capture from choose_capture; one utterance at a time; unicast Ended; no audio or text on the bus"
    )
}
