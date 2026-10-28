//! Playing a flow in a fresh world.

use super::{Evidence, Flow, FlowReport, UndoCheck, judge, setup, transcript};
use crate::hostile::at_rest;
use crate::warm::WarmFault;
use crate::warm_bus::warm_world;
use companion_wire::AnswerWire;
use docket_accept::confirm::ByEffect;
use docket_accept::drive::Launcher;
use docket_accept::keys_home::KeysHome;
use docket_accept::provider::{Focus, Sending};
use docket_accept::things::App;
use docket_accept::world::{Binaries, Consent, ModelSource, Options, TapMode, World};
use docket_core::ModelExchange;
use docket_session::SheetChoice;
use docket_wire::JournalFilter;
use std::time::Duration;

pub(crate) fn exchanges_of(world: &World) -> Vec<ModelExchange> {
    std::fs::read_to_string(world.dir.path().join("model.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

pub(crate) async fn undo_held(launcher: &Launcher, world: &World) -> UndoCheck {
    if world.app.messages().is_empty() {
        return UndoCheck::NothingHeld;
    }
    let journal = docket_accept::world::within(
        "intentd's journal",
        launcher.intents.journal(JournalFilter {
            run: None,
            session: None,
            limit: porter_ids::Count(10),
        }),
    )
    .await;
    let Some(entry) = journal.ok().and_then(|j| j.into_iter().next()) else {
        return UndoCheck::Failed("the journal has no row for the held message".to_owned());
    };
    match docket_accept::world::within("intentd's undo", launcher.intents.undo(entry.id)).await {
        Ok(Ok(_))
            if world
                .app
                .messages()
                .iter()
                .all(|m| m.state == Sending::Cancelled) =>
        {
            UndoCheck::Cancelled
        }
        other => UndoCheck::Failed(format!("{other:?}")),
    }
}

/// What a play of the person's words observes, and the daemons' logs.
pub(crate) struct Played {
    pub evidence: Evidence,
    pub logs: String,
}

/// What a play is told: the person's grants, what they do with a sheet, and their words.
pub(crate) struct Script<'a> {
    pub consent: Consent,
    pub verdict: SheetChoice,
    pub by_effect: Option<ByEffect>,
    pub focus: Focus,
    pub app: App,
    pub prompt: &'a str,
}

/// Plays `script` in a fresh world over `model`, with the daemons' model tap on and the scratch
/// root kept under `keep_in`, until `stop` says the answer has come to rest.
pub(crate) async fn observe(
    binaries: &Binaries,
    script: Script<'_>,
    model: &ModelSource,
    (keep_in, catalog): (Option<std::path::PathBuf>, Option<std::path::PathBuf>),
    keys_home: Option<&KeysHome>,
    patience: Duration,
    stop: impl Fn(&AnswerWire) -> bool,
) -> Result<Played, WarmFault> {
    let options = Options {
        keep_in,
        tap: TapMode::On,
        keys_home: keys_home.cloned(),
        catalog,
        focus: script.focus,
        app: script.app,
        ..Options::default()
    };
    let world = World::start_model(binaries, script.consent, model, &options).await;
    warm_world(&world, model, patience).await?;
    match script.by_effect {
        Some(rule) => world.sheet.will_by_effect(rule),
        None => world.sheet.will(script.verdict),
    }
    let launcher = Launcher::of(&world).await.patient(patience);
    let launcher = match script.focus {
        Focus::Nowhere => launcher,
        Focus::Thread(_) => launcher.summoned_from(
            porter_core::AppName::parse("org.quire.Mail").expect("`org.quire.Mail` is an app name"),
        ),
    };
    let opened = launcher.open().await;
    let mut answer = launcher.say(&opened, script.prompt).await;
    let answered = answer.try_history_until(stop).await;
    let undo = undo_held(&launcher, &world).await;
    let evidence = Evidence {
        answer: answered,
        sheets: world.sheet.shown(),
        messages: world.app.messages(),
        performed: world.app.performed(),
        threads_read: world.app.threads_read(),
        exchanges: exchanges_of(&world),
        undo,
    };
    Ok(Played {
        evidence,
        logs: world.logs(),
    })
}

/// Plays `flow` in a fresh world over `model`, with the daemons' model tap on and the scratch
/// root kept under `options.keep_in`.
pub async fn run_flow(
    binaries: &Binaries,
    flow: Flow,
    model: &ModelSource,
    keep_in: Option<std::path::PathBuf>,
    catalog: Option<std::path::PathBuf>,
    keys_home: Option<&KeysHome>,
    patience: Duration,
) -> FlowReport {
    let script = Script {
        consent: flow.consent(),
        verdict: flow.verdict(),
        by_effect: flow.by_effect(),
        focus: flow.focus(),
        app: flow.app(),
        prompt: flow.prompt(),
    };
    let played = match observe(
        binaries,
        script,
        model,
        (keep_in, catalog),
        keys_home,
        patience,
        at_rest,
    )
    .await
    {
        Ok(played) => played,
        Err(fault) => {
            return FlowReport {
                flow,
                transcript: format!("warm-up failed: {fault}\n"),
                failures: vec![setup(&fault)],
                logs: String::new(),
            };
        }
    };
    let failures = judge(flow, &played.evidence);
    FlowReport {
        flow,
        transcript: transcript(flow, &played.evidence, &failures),
        failures,
        logs: played.logs,
    }
}
