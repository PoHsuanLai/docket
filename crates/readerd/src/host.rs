//! The reader's host: the one place the key to open quarantined text is made, in the reader
//! process. A planner's crates never call `ReaderKey::for_reader_host`.

use prov::{Labelled, Quarantined, ReaderKey};

/// Holds the key that opens quarantined text.
#[derive(Debug)]
pub struct ReaderHost {
    key: ReaderKey,
}

impl ReaderHost {
    /// Starts the host. Only readerd's `main` does.
    pub fn start() -> Self {
        Self {
            key: ReaderKey::for_reader_host(),
        }
    }

    /// Opens quarantined text for the reader model, label kept.
    pub fn open(&self, text: Quarantined<String>) -> Labelled<String> {
        text.open(&self.key)
    }
}
