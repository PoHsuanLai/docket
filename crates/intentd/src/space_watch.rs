//! Keeps consent in step with the desktop's Spaces. When accountd says a Space was removed,
//! every grant scoped to it is dropped and, when the person chose to delete them, memory is asked to; no
//! grant is moved to another Space. A Space removed while the daemon was away (or while the
//! bus was) is found by comparing the grant file with the registry's list each time the
//! subscription is made, so a restart drops what the removal left behind.

use crate::grants::FileGrants;
use docket_core::{AuditRecord, Ended, KnownSpaces};
use docket_dbus::BusConnection;
use docket_router::{Clock, EventSink, MemoryLink, Router, Seams};
use futures_util::StreamExt;
use porter_client::{Spaces, SpacesError};
use porter_core::{DesktopSpace, SpaceChange};
use prov::SpaceId;
use std::sync::Arc;
use std::time::Duration;

/// What the keeper does with the memories of a removed Space. Memory moves them to the apps
/// that wrote them on its own; only the person's choice to delete them makes the keeper ask.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovedMemories {
    /// Memory's own handling stands.
    LeaveToMemory,
    /// Ask memory to delete them with the Space.
    Delete,
}

/// Follows accountd's Spaces for one router.
pub struct SpaceKeeper<S: Seams> {
    router: Arc<Router<S>>,
    memories: RemovedMemories,
    /// How long to wait before subscribing again after accountd is lost.
    retry: Duration,
}

impl<S: Seams> std::fmt::Debug for SpaceKeeper<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpaceKeeper")
            .field("retry", &self.retry)
            .finish_non_exhaustive()
    }
}

impl<S: Seams<Grants = FileGrants>> SpaceKeeper<S> {
    /// A keeper for `router`'s consent file.
    pub fn new(router: Arc<Router<S>>, memories: RemovedMemories, retry: Duration) -> Self {
        Self {
            router,
            memories,
            retry,
        }
    }

    fn said(&self, settled: Result<Vec<Ended>, GrantFileError>) {
        match settled {
            Ok(ended) => {
                let at = self.router.seams.clock().now();
                ended.into_iter().for_each(|ended| {
                    self.router
                        .seams
                        .sink()
                        .append(AuditRecord::GrantsEnded { at, ended });
                });
            }
            Err(why) => eprintln!("intentd: {why}"),
        }
    }

    /// Drops what the registry no longer holds.
    async fn sweep(&self, spaces: &Spaces) -> Result<(), SpacesError> {
        let known = KnownSpaces::of(spaces.list().await?.into_iter().map(|r| r.id));
        self.said(self.router.seams.grants().reconcile_with(&known));
        Ok(())
    }

    /// Ends the consent of a Space that was removed, and has memory delete its records.
    async fn removed(&self, space: DesktopSpace) {
        let id = SpaceId::linked(&space);
        self.said(self.router.seams.grants().end_space(&id));
        match self.memories {
            RemovedMemories::Delete => {
                self.router.seams.memory().erase_space(&id).await;
            }
            RemovedMemories::LeaveToMemory => {}
        }
    }

    /// Subscribes, settles what is already out of date, then follows the changes until the
    /// stream or accountd ends.
    async fn follow(&self, spaces: &Spaces) -> Result<(), SpacesError> {
        let mut changes = spaces.watch().await?;
        self.sweep(spaces).await?;
        while let Some(change) = changes.next().await {
            match change {
                Ok((space, SpaceChange::Removed)) => self.removed(space).await,
                Ok(_) => {}
                Err(why) => eprintln!("intentd: a Space change was not understood: {why}"),
            }
        }
        Ok(())
    }

    /// Follows the Spaces on `bus` until the task is dropped; subscribes again when accountd
    /// goes away.
    pub async fn run(self, bus: BusConnection) {
        loop {
            let ended = match Spaces::connect(&bus).await {
                Ok(spaces) => self.follow(&spaces).await,
                Err(why) => Err(why),
            };
            if let Err(why) = ended {
                eprintln!("intentd: the Spaces are not followed for now: {why}");
            }
            tokio::time::sleep(self.retry).await;
        }
    }
}
