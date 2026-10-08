//! `.Session.Stored`: the durable log of sessions, read through the router. The log is the
//! router's to read (memoryd answers its reads for the router and the shell), so an edge that
//! lists, loads or forks stored sessions asks here. Only the sessions the caller may bring back
//! are listed or read (`may_restore`); any other is answered as one the log does not hold.

use crate::Router;
use crate::seams::Seams;
use docket_core::{
    CallerId, CallerRole, IntentsReply, StoredAsk, StoredRow, StoredView, WireRefusal,
};
use docket_session::{
    Claimant, Logged, PageSize, Seq, SessionLog, child_names, fork, forks_of, may_restore,
    resume_plan,
};
use porter_core::Count;
use prov::SessionId;

/// The most rows one page holds, whatever is asked.
const MOST: u32 = 200;

impl<S: Seams> Router<S> {
    /// Whether `claim` may bring `session` back, from its whole log.
    async fn claimable(&self, session: &SessionId, claim: Claimant<'_>) -> bool {
        let Ok(rows) = self.rows_of(session).await else {
            return false;
        };
        resume_plan(&rows).is_ok_and(|plan| may_restore(claim, plan.opening.opener.as_ref()))
    }

    pub(crate) async fn session_stored(
        &self,
        caller: &CallerId,
        role: CallerRole,
        ask: StoredAsk,
    ) -> IntentsReply {
        let claim = Claimant {
            role,
            app: &caller.app.name,
        };
        match ask {
            StoredAsk::List => {
                let Ok(all) = self.seams.log().sessions().await else {
                    return IntentsReply::Refused(WireRefusal::NoSuchSession);
                };
                let mut mine = Vec::new();
                for session in all {
                    if self.claimable(&session, claim).await {
                        mine.push(session);
                    }
                }
                IntentsReply::Stored(StoredView::Sessions(mine))
            }
            StoredAsk::Fork { session, at } => self.stored_fork(&session, at, claim).await,
            StoredAsk::Rows {
                session,
                from,
                size,
            } => {
                if !self.claimable(&session, claim).await {
                    return IntentsReply::Refused(WireRefusal::NoSuchSession);
                }
                let size = PageSize(Count(size.clamp(1, MOST)));
                match self.seams.log().page(&session, from.map(Seq), size).await {
                    Ok(page) => IntentsReply::Stored(StoredView::Rows {
                        rows: page.rows.iter().filter_map(row).collect(),
                        next: page.next.map(|s| s.0),
                    }),
                    Err(_) => IntentsReply::Refused(WireRefusal::NoSuchSession),
                }
            }
        }
    }
}

impl<S: Seams> Router<S> {
    /// Writes a new session that keeps the parent's log up to and including row `at`. The caller
    /// must be able to bring the parent back; the child is the parent's in everything the
    /// restore rule reads (the opener is copied), so whoever may restore the one may restore the
    /// other. The router writes the child's whole log before it names it.
    async fn stored_fork(&self, parent: &SessionId, at: u64, claim: Claimant<'_>) -> IntentsReply {
        let gone = IntentsReply::Refused(WireRefusal::NoSuchSession);
        if !self.claimable(parent, claim).await {
            return gone;
        }
        let (Ok(rows), Ok(all)) = (
            self.rows_of(parent).await,
            self.seams.log().sessions().await,
        ) else {
            return gone;
        };
        let Ok(plan) = resume_plan(&rows) else {
            return gone;
        };
        let Some((child, task)) = child_names(parent, &plan.opening.task, forks_of(parent, &all))
        else {
            return IntentsReply::Refused(WireRefusal::Malformed);
        };
        let Ok(entries) = fork(parent, &rows, Seq(at), task) else {
            return IntentsReply::Refused(WireRefusal::Malformed);
        };
        for (n, entry) in entries.iter().enumerate() {
            if self
                .seams
                .log()
                .append(&child, Seq(n as u64), entry)
                .await
                .is_err()
            {
                return gone;
            }
        }
        IntentsReply::Stored(StoredView::Forked(child))
    }
}

fn row(logged: &Logged) -> Option<StoredRow> {
    Some(StoredRow {
        seq: logged.seq.0,
        json: serde_json::to_string(logged).ok()?,
    })
}
