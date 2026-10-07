//! The audit trail's way to memory, and to a file between runs: the host writes what the router
//! decided into memory after each turn, and what memory cannot take waits in a bounded queue
//! that a file the app names keeps across a restart.

use crate::agent::InAppAgent;
use crate::audit_file::AuditFileError;
use crate::sheet::ConfirmSheet;
use action_review::Reviewer;
use docket_client::{ContextSource, IntentProvider};
use docket_core::{PolicyWriter, Reader};
use docket_memory::Report;
use docket_router::{Clock, GrantStore, MemoryLink};
use porter_client::Transport as ModelTransport;

impl<P, C, T, R, M: ModelTransport, K, G, Y, W, D> InAppAgent<P, C, T, R, M, K, G, Y, W, D>
where
    P: IntentProvider + 'static,
    C: ContextSource + 'static,
    T: ConfirmSheet + 'static,
    R: Reviewer + 'static,
    K: Clock + Clone + 'static,
    G: GrantStore + 'static,
    Y: MemoryLink + 'static,
    W: PolicyWriter + 'static,
    D: Reader + 'static,
{
    /// Writes the audit records waiting in the buffer into memory (as almanac records in the
    /// agent's Space, the task's episode among them). What memory cannot take, because it is
    /// away or busy, stays queued for the next flush, and in the audit file when the app named
    /// one; the report says what happened. `ask` does this at the end of every turn under
    /// [`AuditTo::Memory`](crate::AuditTo); an app may also call it before it quits, and on
    /// start to write what an earlier run left in the file.
    pub async fn flush_audit(&mut self) -> Report {
        let space = self.space.clone();
        let report = self
            .audit_state
            .flush(
                &self.router.seams.memory,
                self.router.seams.sink.queue(),
                move |_| Some(space.clone()),
            )
            .await;
        self.persist_audit();
        report
    }

    /// The last write of the audit file that failed, if one did; asking clears it.
    pub fn take_audit_fault(&mut self) -> Option<AuditFileError> {
        self.audit_fault.take()
    }

    /// Writes the queue as it stands to the audit file, when there is one.
    fn persist_audit(&mut self) {
        if let Some(file) = &self.audit_file {
            self.audit_fault = file.save(&self.router.seams.sink.queue().snapshot()).err();
        }
    }
}
