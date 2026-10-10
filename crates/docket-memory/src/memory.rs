//! The router's memory seam over memoryd.

use almanac_client::{ClientError, Memory, Transport};
use almanac_core::{HistoryFate, MemoryFate, MemoryReply, MemoryRequest, Removal};
use docket_router::{LinkFault, MemoryLink, SpaceMemories};
use prov::SpaceId;

/// Memory, as the router asks it (`Caller::Router`).
#[derive(Debug, Clone)]
pub struct AlmanacMemory<T: Transport> {
    memory: Memory<T>,
}

impl<T: Transport> AlmanacMemory<T> {
    /// Asks memoryd over `transport`.
    pub fn over(transport: T) -> Self {
        Self {
            memory: Memory::over(transport),
        }
    }
}

impl<T: Transport> MemoryLink for AlmanacMemory<T> {
    async fn ask(&self, request: MemoryRequest) -> Result<MemoryReply, LinkFault> {
        match self.memory.ask(request).await {
            Ok(reply) => Ok(reply),
            // A refusal is memoryd's answer, not a failure of the link: the router decides what
            // it means.
            Err(ClientError::Refused(refusal)) => Ok(MemoryReply::Refused(refusal)),
            Err(ClientError::Transport(_)) => Err(LinkFault::Unavailable),
            Err(ClientError::Unexpected) => Err(LinkFault::Malformed),
            // A failure this link does not know yet: the link cannot be trusted to have worked.
            Err(_) => Err(LinkFault::Unavailable),
        }
    }

    async fn erase_space(&self, space: &SpaceId) -> SpaceMemories {
        match self
            .ask(MemoryRequest::RemoveSpace(
                space.clone(),
                // The person chose to delete the memories; the history is a separate choice,
                // made in the removal sheet, and is kept here.
                Removal {
                    memories: MemoryFate::Delete,
                    history: HistoryFate::Keep,
                },
            ))
            .await
        {
            Ok(MemoryReply::Relocated(_)) => SpaceMemories::Deleted,
            Ok(_) | Err(_) => SpaceMemories::Kept,
        }
    }
}
