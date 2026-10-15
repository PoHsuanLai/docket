//! The event fd of one client: a socket pair whose far end goes to the client and whose near end
//! a writer task fills with `VoiceEvent` frames. A client that stops reading costs at most
//! `QUEUE_FRAMES` queued frames: past that the client is closed (it reads to the end of the
//! stream and no more), never the audio path; a client that closed ends the task.

use crate::wire::frame;
use std::os::fd::OwnedFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::AsyncWriteExt;
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use voice_wire::VoiceEvent;

/// How many frames may wait for a client that does not read (about half a minute of levels).
pub(crate) const QUEUE_FRAMES: usize = 1024;

/// Writes events to one client.
#[derive(Debug, Clone)]
pub(crate) struct EventSink {
    tx: mpsc::Sender<Vec<u8>>,
    overflowed: Arc<AtomicBool>,
}

impl EventSink {
    /// A sink and the descriptor of its far end. Needs a tokio runtime.
    pub fn open() -> std::io::Result<(Self, OwnedFd)> {
        let (near, far) = UnixStream::pair()?;
        let (tx, mut rx) = mpsc::channel::<Vec<u8>>(QUEUE_FRAMES);
        let overflowed = Arc::new(AtomicBool::new(false));
        let flag = overflowed.clone();
        tokio::spawn(async move {
            let mut near = near;
            while let Some(bytes) = rx.recv().await {
                if flag.load(Ordering::SeqCst) || near.write_all(&bytes).await.is_err() {
                    break;
                }
            }
            let _ = near.shutdown().await;
        });
        Ok((Self { tx, overflowed }, OwnedFd::from(far.into_std()?)))
    }

    /// Queues one event; a full queue closes the client instead of growing.
    pub fn send(&self, event: &VoiceEvent) {
        if let Ok(bytes) = frame(event)
            && self.tx.try_send(bytes).is_err()
        {
            self.overflowed.store(true, Ordering::SeqCst);
        }
    }
}
