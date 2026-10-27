//! The write lock of a grant store: one writer at a time, held by the store that owns it.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

/// One writer at a time: a grant recorded while another is being written is added to the file the
/// first left, not to the one it read before. A clone shares the lock; there is no global one.
#[derive(Debug, Clone, Default)]
pub struct WriteLock(Arc<Mutex<()>>);

impl WriteLock {
    /// A lock of its own.
    pub fn new() -> Self {
        Self::default()
    }

    /// Waits for the lock; it is let go when the guard is dropped. A writer that panicked
    /// leaves nothing half-written (the file is replaced whole), so its poison is ignored.
    pub fn hold(&self) -> MutexGuard<'_, ()> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}
