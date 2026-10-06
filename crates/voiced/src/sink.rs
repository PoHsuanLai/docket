//! The event fd of one client: a socket pair whose far end goes to the client and whose near end
//! a writer task fills with `VoiceEvent` frames. A client that stops reading costs memory in
//! frames of text and levels only, never the audio path; a client that closed ends the task.

use crate::wire::frame;
use std::os::fd::OwnedFd;
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use voice_wire::VoiceEvent;

/// Writes events to one client.
#[derive(Debug, Clone)]
pub(crate) struct EventSink {
    tx: mpsc::UnboundedSender<Vec<u8>>,
}

impl EventSink {
    /// A sink and the descriptor of its far end. Needs a tokio runtime.
    pub fn open() -> std::io::Result<(Self, OwnedFd)> {
        let (near, far) = UnixStream::pair()?;
        let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
        tokio::spawn(async move {
            let mut near = near;
            while let Some(bytes) = rx.recv().await {
                if near.write_all(&bytes).await.is_err() {
                    break;
                }
            }
            let _ = near.shutdown().await;
        });
        Ok((Self { tx }, OwnedFd::from(far.into_std()?)))
    }

    /// Queues one event.
    pub fn send(&self, event: &VoiceEvent) {
        if let Ok(bytes) = frame(event) {
            let _ = self.tx.send(bytes);
        }
    }
}
