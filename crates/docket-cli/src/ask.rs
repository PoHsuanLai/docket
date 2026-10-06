//! `quire-do ask "<text>"`: the person at a terminal talks to the companion, the way sill's
//! launcher does. Open a conversation, record the turn with intentd (`Session.Turn`, which a
//! terminal may make: it is recorded as `TurnSource::Terminal`), hand it to companiond, and
//! follow the answer object until it settles.
//!
//! `quire-do` is not `Confirm1`: when the answer needs the person for a confirmation it prints
//! what is waiting and ends with its own exit code, and the confirmation stays on the sheet in the
//! shell. Nothing here ever answers one.

use crate::ask_render;
use crate::exec::Style;
use crate::exit::{Exit, Failure, of_client};
use crate::{JsonFlag, Report, Stdout};
use companion_client::{CompanionTransport, Follow};
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, AskWire, RefusalWire};
use docket_client::{ClientError, Intents, Transport, TransportError};
use docket_core::{
    ContextKeep, Keep, Origin, SessionOpen, SessionOpened, TurnIn, TurnSource, TurnVia, UserTurn,
    WindowKey,
};
use prov::{AgentRef, SessionId, SpaceId, UnixSeconds};
use serde_json::json;

/// The window a terminal asks from: the companion anchors a confirmation to it.
const WINDOW: &str = "terminal";

/// Nothing of the screen is kept with a prompt typed in a terminal.
fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

fn unavailable(error: &TransportError) -> Failure {
    match error {
        TransportError::Closed => Failure::new(
            Exit::Unavailable,
            "the companion is not running (companiond is not installed, or did not start)",
        ),
        other => Failure::new(Exit::Unavailable, format!("the companion: {other}")),
    }
}

/// The turn the router recorded, as the companion is handed it.
fn ask_of(
    opened: &SessionOpened,
    id: docket_core::TurnId,
    text: &str,
    now: UnixSeconds,
) -> AskWire {
    AskWire {
        session: opened.session.clone(),
        turn: UserTurn {
            id,
            text: text.to_owned(),
            at: now,
            from: TurnSource::Terminal,
            via: TurnVia::Typed,
        },
        keep: keep_nothing(),
        parent_window: WindowKey::parse(WINDOW).expect("`terminal` is a window key"),
        app: None,
    }
}

/// How the run ends once the answer settled, with what it showed.
fn exit_of(answer: &AnswerWire) -> Exit {
    match (&answer.phase, &answer.body) {
        (AnswerPhase::NeedsYou(_), _) => Exit::NeedsYou,
        (AnswerPhase::Done, AnswerBody::Refused(why)) => refused(why),
        (AnswerPhase::Done, _) => Exit::Done,
        (_, AnswerBody::Refused(why)) => refused(why),
        (AnswerPhase::Cancelled, _) => Exit::Declined,
        _ => Exit::AppFailed,
    }
}

fn refused(why: &RefusalWire) -> Exit {
    match why {
        RefusalWire::NeedsCloud(_)
        | RefusalWire::NotAllowed(_)
        | RefusalWire::NoWay { .. }
        | RefusalWire::OverBudget(_) => Exit::Refused,
        RefusalWire::Failed(_) => Exit::AppFailed,
    }
}

fn report(answer: &AnswerWire, style: Style) -> Report {
    let exit = exit_of(answer);
    let stdout = match style {
        Style::Human => format!("{}\n", ask_render::human(answer)),
        Style::Json => format!(
            "{}\n",
            json!({
                "vocab": docket_core::IntentsVocab::CURRENT,
                "answer": answer,
                "exit": exit.code(),
            })
        ),
    };
    Report {
        stdout,
        stderr: String::new(),
        exit,
    }
}

/// Follows the answer until it is no longer working.
async fn settled(answer: &mut impl Follow) -> Result<AnswerWire, Failure> {
    let mut last = None;
    while let Some(view) = answer.next().await.map_err(|e| unavailable(&e))? {
        let working = matches!(view.phase, AnswerPhase::Thinking | AnswerPhase::Streaming);
        last = Some(view);
        if !working {
            break;
        }
    }
    last.ok_or_else(|| Failure::new(Exit::Unavailable, "the companion dropped the answer"))
}

async fn turn<T: Transport>(
    intents: &Intents<T>,
    session: &SessionId,
    text: &str,
) -> Result<docket_core::TurnId, Failure> {
    intents
        .session_turn(
            session.clone(),
            TurnIn {
                text: text.to_owned(),
                origin: Origin::Cli,
                keep: keep_nothing(),
                via: TurnVia::Typed,
            },
        )
        .await
        .map_err(|e: ClientError| of_client(&e))
}

async fn run<T: Transport, C: CompanionTransport>(
    intents: &Intents<T>,
    companion: &C,
    text: &str,
    space: SpaceId,
    now: UnixSeconds,
) -> Result<(AnswerWire, SessionId), Failure> {
    let open = SessionOpen {
        space,
        agent: AgentRef::Companion,
        parent: None,
    };
    let opened = companion.open(&open).await.map_err(|e| unavailable(&e))?;
    let id = turn(intents, &opened.session, text).await?;
    let mut answer = companion
        .ask(&ask_of(&opened, id, text, now))
        .await
        .map_err(|e| unavailable(&e))?;
    Ok((settled(&mut answer).await?, opened.session))
}

/// The whole of `quire-do ask`: talk to the companion, print the answer, and say how it ended.
/// A conversation that finished is closed; one that waits for a confirmation is left open, for
/// the sheet.
pub async fn ask<T: Transport, C: CompanionTransport>(
    intents: &Intents<T>,
    companion: &C,
    text: &str,
    space: SpaceId,
    now: UnixSeconds,
    stdout: Stdout,
    json: JsonFlag,
) -> Report {
    let style = match (json, stdout) {
        (JsonFlag::Asked, _) | (_, Stdout::Pipe) => Style::Json,
        (JsonFlag::Auto, Stdout::Tty) => Style::Human,
    };
    match run(intents, companion, text, space, now).await {
        Ok((answer, session)) => {
            if !matches!(answer.phase, AnswerPhase::NeedsYou(_)) {
                let _ = companion.close(&session).await;
            }
            report(&answer, style)
        }
        Err(failure) => Report::failed(&failure, style),
    }
}
