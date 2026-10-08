//! `.Session.Stored`: the durable log of sessions, read through the router. The log is the
//! router's to read (memoryd answers its reads for the router and the shell), so an edge that
//! lists, loads or forks stored sessions asks here. Only the sessions the caller may bring back
//! are listed or read (`may_restore`); any other is answered as one the log does not hold.

use crate::Router;
use crate::seams::Seams;
use docket_core::{
    CallerId, CallerRole, IntentsReply, StoredAsk, StoredRow, StoredView, WireRefusal,
};
use docket_session::{Claimant, Logged, PageSize, Seq, SessionLog, may_restore, resume_plan};
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

fn row(logged: &Logged) -> Option<StoredRow> {
    Some(StoredRow {
        seq: logged.seq.0,
        json: serde_json::to_string(logged).ok()?,
    })
}
