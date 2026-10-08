//! `quire-do sessions`: list, load and fork the sessions the terminal may bring back. All of it
//! goes through `Session.Stored`: the router lists and pages only what the caller's role and app
//! may restore (the same opener rule as a restore: a terminal brings back the sessions its own
//! app opened) and answers any other session as one it does not hold. Nothing here writes a log;
//! a fork is the router's to write.

use crate::exec::Printed;
use crate::exit::{Failure, of_client};
use docket_client::{Intents, Transport};
use docket_core::{IntentsVocab, StoredAsk, StoredView};
use docket_session::{Logged, ResumePlan, SessionEntry, Standing, export, resume_plan, to_json};
use prov::SessionId;
use serde_json::{Value, json};

/// What `quire-do sessions` was asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionsCmd {
    /// `sessions`: the sessions that may be brought back.
    List,
    /// `sessions load <id>`: one session's turns and calls.
    Load(SessionId),
    /// `sessions fork <id> [--at <row>]`: a new session that keeps the log up to a row (the last
    /// one by default).
    Fork {
        /// The session to fork.
        session: SessionId,
        /// The last row the fork keeps.
        at: Option<u64>,
    },
}

/// Every row of `session`, oldest first, read through `Session.Stored` a page at a time.
async fn rows<T: Transport>(
    intents: &Intents<T>,
    session: &SessionId,
) -> Result<Vec<Logged>, Failure> {
    const PAGE: u32 = 200;
    let mut all = Vec::new();
    let mut from = None;
    loop {
        let ask = StoredAsk::Rows {
            session: session.clone(),
            from,
            size: PAGE,
        };
        match intents
            .session_stored(ask)
            .await
            .map_err(|e| of_client(&e))?
        {
            StoredView::Rows { rows, next } => {
                for row in rows {
                    let logged = serde_json::from_str::<Logged>(&row.json).map_err(|_| {
                        Failure::new(crate::exit::Exit::Unavailable, "a stored row is unreadable")
                    })?;
                    all.push(logged);
                }
                match next {
                    Some(next) => from = Some(next),
                    None => return Ok(all),
                }
            }
            StoredView::Sessions(_) | StoredView::Forked(_) => {
                return Err(Failure::new(
                    crate::exit::Exit::Unavailable,
                    "the router answered something else than rows",
                ));
            }
        }
    }
}

fn standing_word(standing: &Standing) -> &'static str {
    match standing {
        Standing::Open => "open",
        Standing::Paused(_) => "paused",
        Standing::Closed(_) => "closed",
        Standing::Blocked(_) => "unreadable",
    }
}

fn plan_json(session: &SessionId, plan: &ResumePlan) -> Value {
    json!({
        "session": session,
        "space": plan.opening.space,
        "agent": plan.opening.agent,
        "turns": plan.turns.len(),
        "standing": standing_word(&plan.standing),
    })
}

fn plan_line(session: &SessionId, plan: &ResumePlan) -> String {
    let agent = plan.opening.agent.as_ref().map_or_else(
        || "companion".to_owned(),
        |a| format!("{a:?}").to_lowercase(),
    );
    let turns = plan.turns.len();
    format!(
        "{session}  {}  {agent}  {turns} turn{}  {}",
        plan.opening.space,
        if turns == 1 { "" } else { "s" },
        standing_word(&plan.standing)
    )
}

/// `sessions`: one line each, oldest first.
pub async fn list<T: Transport>(intents: &Intents<T>) -> Result<Printed, Failure> {
    let StoredView::Sessions(all) = intents
        .session_stored(StoredAsk::List)
        .await
        .map_err(|e| of_client(&e))?
    else {
        return Err(Failure::new(
            crate::exit::Exit::Unavailable,
            "the router answered something else than a list",
        ));
    };
    let mut lines = Vec::new();
    let mut found = Vec::new();
    for session in all {
        // A session whose log has no plan (swept, or not whole) is left out of the list.
        if let Ok(plan) = resume_plan(&rows(intents, &session).await?) {
            lines.push(plan_line(&session, &plan));
            found.push(plan_json(&session, &plan));
        }
    }
    let human = if lines.is_empty() {
        "no sessions".to_owned()
    } else {
        lines.join("\n")
    };
    Ok(Printed {
        human,
        json: json!({ "vocab": IntentsVocab::CURRENT, "sessions": found }),
    })
}

/// The person's words and the calls of one entry, as a line; entries that say nothing a person
/// reads (policy, handles, budgets) have none.
fn entry_line(entry: &SessionEntry) -> Option<String> {
    match entry {
        SessionEntry::Opened(o) => Some(format!("opened in {}", o.space)),
        SessionEntry::Turn(t) => Some(format!("you: {}", t.text)),
        SessionEntry::Call(c) => Some(format!("  call {} ({:?})", c.action.name, c.effect)),
        SessionEntry::Closed(cause) => Some(format!("closed ({cause:?})")),
        _ => None,
    }
}

/// `sessions load <id>`: the session's turns and calls; `--json` is the export document.
pub async fn load<T: Transport>(
    intents: &Intents<T>,
    session: &SessionId,
) -> Result<Printed, Failure> {
    let all = rows(intents, session).await?;
    let document = export(session, &all)
        .map_err(|e| Failure::new(crate::exit::Exit::Unavailable, e.to_string()))?;
    let human = document
        .entries
        .iter()
        .filter_map(entry_line)
        .collect::<Vec<_>>()
        .join("\n");
    let json = to_json(&document)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or(Value::Null);
    Ok(Printed { human, json })
}

/// `sessions fork <id> [--at <row>]`: asks the router to write the fork and names it.
pub async fn fork<T: Transport>(
    intents: &Intents<T>,
    session: &SessionId,
    at: Option<u64>,
) -> Result<Printed, Failure> {
    let at = match at {
        Some(at) => at,
        None => rows(intents, session)
            .await?
            .last()
            .map(|row| row.seq.0)
            .ok_or_else(|| Failure::new(crate::exit::Exit::Usage, "that session has no rows"))?,
    };
    let view = intents
        .session_stored(StoredAsk::Fork {
            session: session.clone(),
            at,
        })
        .await
        .map_err(|e| of_client(&e))?;
    let StoredView::Forked(child) = view else {
        return Err(Failure::new(
            crate::exit::Exit::Unavailable,
            "the router answered something else than a fork",
        ));
    };
    Ok(Printed {
        human: format!("{child}"),
        json: json!({ "vocab": IntentsVocab::CURRENT, "forked": session, "session": child, "at": at }),
    })
}
