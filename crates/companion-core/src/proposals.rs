//! The routines proposed and not yet accepted or declined. They are kept in memory only: a
//! restart of the assistant forgets them, and nothing runs from one before the person accepts.

use companion_wire::{ProposalId, RoutineForm, RoutineProposal};

/// The most proposals kept waiting; more are refused until the person answers some.
pub const MOST_WAITING: usize = 20;

/// The proposals waiting for the person, oldest first.
#[derive(Debug, Default)]
pub struct Proposals {
    last: u64,
    waiting: Vec<RoutineProposal>,
}

impl Proposals {
    /// Keeps a proposal and gives its id; none when too many already wait.
    pub fn add(&mut self, form: RoutineForm, why: String) -> Option<ProposalId> {
        if self.waiting.len() >= MOST_WAITING {
            return None;
        }
        self.last += 1;
        let id = ProposalId(self.last);
        self.waiting.push(RoutineProposal {
            id,
            helper: form.helper.clone(),
            form,
            why,
        });
        Some(id)
    }

    /// Everything that waits.
    pub fn list(&self) -> Vec<RoutineProposal> {
        self.waiting.clone()
    }

    /// Takes one off the list.
    pub fn take(&mut self, id: ProposalId) -> Option<RoutineProposal> {
        let at = self.waiting.iter().position(|p| p.id == id)?;
        Some(self.waiting.remove(at))
    }
}
