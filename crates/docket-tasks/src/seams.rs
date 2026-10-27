//! The seam the task model is generic over beyond the router link, the model transport and the
//! clock (`docket_core::Clock`): the surface the tasks are shown on. It reads no environment:
//! companiond hands in a bus-facing surface, an app hands in its own.

use crate::shared::Change;
use prov::TaskId;
use std::future::Future;

/// Where the tasks are shown: what is told when state changes, how an interactive request cuts
/// into background work, and what an answer is called out there.
pub trait Surface: Default {
    /// What a subscriber holds to hear the changes.
    type Changes;

    /// Something changed (content-free: a reader asks for the content).
    fn changed(&self, change: Change);

    /// A subscription to the changes.
    fn changes(&self) -> Self::Changes;

    /// An interactive request is starting: background work yields now.
    fn interrupt(&self);

    /// Resolves when an interactive request starts.
    fn interrupted(&self) -> impl Future<Output = ()> + Send;

    /// The address of a task's answer on this surface (an object path on a bus).
    fn answer_path(&self, task: &TaskId) -> String;
}

/// A surface nobody listens to: no subscription, an interruption that never comes (one caller at a
/// time cannot interrupt itself), and the task's id for an address.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Quiet;

impl Surface for Quiet {
    type Changes = ();

    fn changed(&self, _change: Change) {}

    fn changes(&self) -> Self::Changes {}

    fn interrupt(&self) {}

    fn interrupted(&self) -> impl Future<Output = ()> + Send {
        std::future::pending()
    }

    fn answer_path(&self, task: &TaskId) -> String {
        task.as_str().to_owned()
    }
}
