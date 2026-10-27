//! How the hub starts the tasks that run its jobs. The hub owns what a job needs; whether a task
//! runs on tokio or on another executor is the caller's choice, so the hub asks for it through
//! this seam. `TokioSpawn` (feature `tokio`) is the one for a tokio runtime.

use companion_core::Finished;
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

/// A task that returns nothing.
pub type Task = Pin<Box<dyn Future<Output = ()> + Send>>;

/// The task that runs one job and gives its result.
pub type Work = Pin<Box<dyn Future<Output = Finished> + Send>>;

/// What is done with how a job's task ended: the result, or how it did not give one.
pub type Watch = Box<dyn FnOnce(Result<Finished, Fate>) -> Task + Send>;

/// How a job's task ended without a result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fate {
    /// It was cut off.
    Cut,
    /// It panicked.
    Panicked,
}

/// What ends a running task before it is done.
#[derive(Clone)]
pub struct Cutoff {
    cut: Arc<dyn Fn() + Send + Sync>,
}

impl Cutoff {
    /// A cutoff that does `cut`.
    pub fn new(cut: impl Fn() + Send + Sync + 'static) -> Self {
        Self { cut: Arc::new(cut) }
    }

    /// Ends the task, if it is still running.
    pub fn cut(&self) {
        (self.cut)();
    }
}

impl fmt::Debug for Cutoff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cutoff").finish_non_exhaustive()
    }
}

/// Starts the hub's tasks.
pub trait Spawn: Send + Sync + 'static {
    /// Runs `work` as a task of its own, and a second task that awaits it and hands `watch` how it
    /// ended (a panic in `work` reaches only `watch`). The cutoff of the first task.
    fn run_watched(&self, work: Work, watch: Watch) -> Cutoff;

    /// Runs `task` on its own.
    fn detach(&self, task: Task);

    /// Runs `task` on its own once `wait` has passed.
    fn detach_after(&self, wait: Duration, task: Task);
}

#[cfg(feature = "tokio")]
pub use tokio_spawn::TokioSpawn;

#[cfg(feature = "tokio")]
mod tokio_spawn {
    use super::{Cutoff, Fate, Spawn, Task, Watch, Work};
    use std::time::Duration;

    /// The tasks of the tokio runtime the caller is in.
    #[derive(Debug, Clone, Copy, Default)]
    pub struct TokioSpawn;

    impl Spawn for TokioSpawn {
        fn run_watched(&self, work: Work, watch: Watch) -> Cutoff {
            let work = tokio::spawn(work);
            let abort = work.abort_handle();
            tokio::spawn(async move {
                let ended = match work.await {
                    Ok(done) => Ok(done),
                    Err(join) if join.is_cancelled() => Err(Fate::Cut),
                    Err(_) => Err(Fate::Panicked),
                };
                watch(ended).await;
            });
            Cutoff::new(move || abort.abort())
        }

        fn detach(&self, task: Task) {
            tokio::spawn(task);
        }

        fn detach_after(&self, wait: Duration, task: Task) {
            tokio::spawn(async move {
                tokio::time::sleep(wait).await;
                task.await;
            });
        }
    }
}
