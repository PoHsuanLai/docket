//! Carrying a command out: ask `Intents1` through the client and turn what comes back into
//! text. Nothing here talks to an app, and nothing here decides: every call is the router's to
//! allow, ask about or refuse.

use crate::args::{CallArgs, RunMode, UndoWhich};
use crate::exit::{Failure, of_client, of_refusal, of_undo};
use crate::outcome::{context_json, dry_run_json, hits_json, outcome_json};
use crate::params::{Reading, StdinSlot, arguments, target};
use crate::resolve::{Apps, slug, visible};
use crate::{render, schema};
use docket_client::{Intents, Transport};
use docket_core::{
    ActionRef, CallRequest, Generation, IntentsVocab, JournalFilter, Origin, SearchAsk,
    SearchScope, UndoId, UndoState, Undoable,
};
use porter_core::Count;
use prov::SessionId;
use serde_json::{Value, json};

/// How the answer is written: for a person, or for a program.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// Lines of text.
    Human,
    /// One JSON document.
    Json,
}

/// What a command prints on success.
pub struct Printed {
    /// The words for a person.
    pub human: String,
    /// The document for a program.
    pub json: Value,
}

impl Printed {
    /// The text for `style`.
    pub fn text(&self, style: Style) -> String {
        match style {
            Style::Human => self.human.clone(),
            Style::Json => self.json.to_string(),
        }
    }
}

/// The apps `quire-do apps` lists.
pub fn apps(apps: &Apps) -> Printed {
    let rows: Vec<Value> = apps
        .all()
        .iter()
        .map(|m| {
            let m = m.manifest();
            json!({
                "slug": slug(&m.app),
                "app": m.app,
                "actions": visible(m).count(),
                "kinds": m.entities.iter().map(|e| &e.kind).collect::<Vec<_>>(),
            })
        })
        .collect();
    Printed {
        human: render::apps(apps),
        json: json!({ "vocab": IntentsVocab::CURRENT, "apps": rows }),
    }
}

/// One app's actions.
pub fn list(apps: &Apps, word: &str) -> Result<Printed, Failure> {
    let app = apps.app(word)?;
    let rows: Vec<Value> = visible(app).map(|a| schema::action_row(app, a)).collect();
    Ok(Printed {
        human: render::list(app),
        json: json!({ "vocab": IntentsVocab::CURRENT, "app": slug(&app.app), "actions": rows }),
    })
}

/// One action's parameters as a JSON Schema.
pub fn describe(apps: &Apps, app: &str, action: &str) -> Result<Printed, Failure> {
    let app = apps.app(app)?;
    let decl = apps
        .action(app, action)
        .ok()
        .filter(|a| a.reach != docket_core::AgentReach::Hidden)
        .ok_or_else(|| Failure::usage(format!("{} has no action {action:?}", slug(&app.app))))?;
    let document = schema::described(app, decl)?;
    let human = format!(
        "{} ({:?}): {}\n{}",
        decl.name,
        decl.effect,
        decl.label,
        serde_json::to_string_pretty(&document["schema"]).unwrap_or_default()
    );
    Ok(Printed {
        human,
        json: document,
    })
}

/// The journal row a finished call left, found by its undo token: what `undo` takes.
async fn undo_id<T: Transport>(
    intents: &Intents<T>,
    token: &docket_core::UndoToken,
) -> Option<UndoId> {
    let rows = intents
        .journal(JournalFilter {
            run: None,
            session: None,
            limit: Count(10),
        })
        .await
        .ok()?;
    rows.iter().find(|e| &e.token == token).map(|e| e.id)
}

/// A call: run it, or with `--dry-run` describe what it would change.
pub async fn call<T: Transport>(
    intents: &Intents<T>,
    apps: &Apps,
    args: &CallArgs,
    stdin: &mut StdinSlot,
) -> Result<Printed, Failure> {
    let app = apps.app(&args.app)?;
    let decl = apps.action(app, &args.action)?;
    let mut reading = Reading {
        apps,
        owner: &app.app,
        stdin,
    };
    let request = CallRequest {
        action: ActionRef {
            app: app.app.clone(),
            name: decl.name.clone(),
        },
        target: target(decl, &reading, &args.targets)?,
        args: arguments(decl, &mut reading, &args.params)?,
        origin: Origin::Cli,
    };
    match args.mode {
        RunMode::DryRun => {
            let preview = intents
                .dry_run(request, args.session.clone())
                .await
                .map_err(|e| of_client(&e))?;
            Ok(Printed {
                human: render::preview(&preview),
                json: dry_run_json(&preview),
            })
        }
        RunMode::Run => {
            let outcome = intents
                .perform(request, args.session.clone(), None)
                .await
                .map_err(|e| of_client(&e))?
                .map_err(|refusal| of_refusal(&refusal))?;
            let undo = match &outcome.undo {
                Undoable::Yes(token) => undo_id(intents, token).await,
                Undoable::No => None,
            };
            Ok(Printed {
                human: render::outcome(&outcome, undo),
                json: outcome_json(&outcome, undo),
            })
        }
    }
}

/// `<app> search <text>`.
pub async fn search<T: Transport>(
    intents: &Intents<T>,
    apps: &Apps,
    word: &str,
    text: &str,
) -> Result<Printed, Failure> {
    let app = apps.app(word)?;
    let all = intents
        .search(SearchAsk {
            text: text.to_owned(),
            scope: SearchScope::Everything,
            generation: Generation(1),
        })
        .await
        .map_err(|e| of_client(&e))?;
    let hits: Vec<_> = all
        .into_iter()
        .filter(|h| h.entity.id.app == app.app)
        .collect();
    Ok(Printed {
        human: render::hits(&hits),
        json: hits_json(&hits),
    })
}

/// `<app> context`: what the app shows now. The terminal has one session of its own, which the
/// router uses whatever id is sent.
pub async fn context<T: Transport>(
    intents: &Intents<T>,
    apps: &Apps,
    word: &str,
) -> Result<Printed, Failure> {
    let app = apps.app(word)?;
    let own = SessionId::parse("s-0").map_err(|_| Failure::usage("no session"))?;
    let view = intents
        .context(own, app.app.clone())
        .await
        .map_err(|e| of_client(&e))?;
    Ok(Printed {
        human: render::context(&view),
        json: context_json(&view),
    })
}

/// `undo <id>` and `undo --last`: only what a terminal did can be undone from one.
pub async fn undo<T: Transport>(
    intents: &Intents<T>,
    which: UndoWhich,
) -> Result<Printed, Failure> {
    let id = match which {
        UndoWhich::Entry(n) => UndoId(n),
        UndoWhich::Last => intents
            .journal(JournalFilter {
                run: None,
                session: None,
                limit: Count(50),
            })
            .await
            .map_err(|e| of_client(&e))?
            .iter()
            .find(|e| e.state == UndoState::Available)
            .map(|e| e.id)
            .ok_or_else(|| Failure::usage("nothing from this terminal can be undone"))?,
    };
    intents
        .undo(id)
        .await
        .map_err(|e| of_client(&e))?
        .map_err(of_undo)?;
    Ok(Printed {
        human: format!("undone: {}", id.0),
        json: json!({ "vocab": IntentsVocab::CURRENT, "undone": { "id": id.0 } }),
    })
}
