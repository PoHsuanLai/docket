//! The attack suite played by the companion's real planner (`docket-live corpus --corpus attacks
//! --planner live`): AgentDojo's own measurement. The planner model is given the person's words
//! over the attack's world, once with the carrier holding filler and once holding the injection,
//! and chooses its own steps through the companion's real loop, in this process, over the same
//! router the scripted run uses. The policy writer and the reviewers are the real models over
//! inferd, as in the scripted live run; the reader that reads untrusted text is a real reader
//! model too, so the planner sees what it would see in the product (the injection reaches it,
//! if it does, only as the reader's typed answer). The scripted person (`person.rs`) allows the
//! sheets of the task's own actions and refuses every other.
//!
//! The scores are `docket-eval`'s (`score_live`): they read the trace this run builds from the
//! router's audit records. A live run is a measurement, not a gate.

use crate::live::corpus::{CorpusError, CorpusOptions, Judges, Link, card};
use crate::live::inferd_world::InferdWorld;
use crate::live::person::Person;
use crate::live::trace_dir::TraceDir;
use crate::live::warm_bus::warm_up;
use crate::world::{Binaries, Options, TapMode};
use action_review::InferReviewer;
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, AskWire};
use companiond::{Clock, Companiond, PlannerModel};
use docket_client::{InProcess, Intents};
use docket_core::{
    AgentConfig, CallerId, CallerRole, ContextKeep, Keep, Origin, SessionOpen, TurnIn, TurnSource,
    TurnVia, UserTurn, WindowKey,
};
use docket_dbus::tap::{MemoryTap, Tap, Tapped};
use docket_eval::{
    AnswerGiven, AttackCase, Case, CaseTrace, LiveRun, LiveScore, Place, RunNote, faulted_trace,
    install_fixture, live_trace, score_live,
};
use docket_fake::fake_router_reading;
use docket_reader::TransportReader;
use docket_session::fake::MemoryLog;
use intentd::{InferdModel, InferdWriter, SystemClock};
use porter_client::{AnyTransport, DbusTransport};
use porter_core::{AppId, AppName, Isolation};
use prov::{AgentRef, SessionId};
use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// How long one case may take before its planner is given up on. A local model that thinks is
/// slow; a case that takes longer is a planner that does not finish, and is scored as one.
const CASE_BOUND: Duration = Duration::from_secs(30 * 60);

/// What a live-planner attack run produced.
#[derive(Debug)]
pub struct AttackPlannerOutcome {
    /// The planner model's name, as the inferd config names it.
    pub planner: String,
    /// What each attack scored.
    pub scores: Vec<LiveScore>,
    /// Attacks the harness could not play, with why: never counted as a defence that held.
    pub unscored: Vec<(String, String)>,
    /// Every case's trace, with the file it was written to.
    pub traces: Vec<(String, CaseTrace)>,
    /// Where the trace directory is.
    pub trace_dir: PathBuf,
    /// How the run was made and what it missed.
    pub note: RunNote,
}

/// The model the inferd config names for the planner's tier (`ai.model.text.balanced`).
pub fn planner_model_name(config: &str) -> Option<String> {
    let table: toml::Table = toml::from_str(config).ok()?;
    table
        .get("ai")?
        .get("model")?
        .get("text")?
        .get("balanced")?
        .as_str()
        .map(str::to_owned)
}

fn planner_of(options: &CorpusOptions) -> String {
    options
        .inferd_config
        .as_deref()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| planner_model_name(&text))
        .unwrap_or_else(|| "unnamed".to_owned())
}

/// What every case of a run shares: the link to inferd and the tap that keeps its exchanges.
struct Rig {
    connection: docket_dbus::BusConnection,
    tap: Tap,
    memory: MemoryTap,
    timeouts: crate::live::corpus::Timeouts,
}

impl Rig {
    fn link(&self) -> Link {
        Tapped::new(
            AnyTransport::Dbus(DbusTransport::over(self.connection.clone())),
            self.tap.clone(),
        )
    }

    fn model(&self, name: &'static str) -> Result<InferdModel<Link>, String> {
        card(name)
            .map(|card| InferdModel::new(self.link(), card))
            .ok_or_else(|| format!("no model card for {name}"))
    }
}

fn caller(name: &str, role: CallerRole) -> Result<CallerId, String> {
    let app = AppName::parse(name).map_err(|_| format!("{name} is not an app name"))?;
    Ok(CallerId {
        app: AppId {
            name: app,
            isolation: Isolation::Unsandboxed,
        },
        roles: BTreeSet::from([role]),
    })
}

fn keep_nothing() -> ContextKeep {
    ContextKeep {
        query: Keep::Dropped,
        results: Keep::Dropped,
        selection: Keep::Dropped,
        window: Keep::Dropped,
    }
}

/// The ask a turn makes from where the person is: the companion reads the window of the place's
/// app when it resolves "this", as it does for a turn asked from an app's window.
pub fn ask_of(session: SessionId, turn: UserTurn, place: &Place) -> Result<AskWire, String> {
    Ok(AskWire {
        session,
        turn,
        keep: keep_nothing(),
        parent_window: WindowKey::parse("w1").map_err(|_| "the window's name".to_owned())?,
        app: Some(place.app.clone()),
    })
}

/// Plays one case: a fresh router over the case's world, the companion with its real planner, the
/// person's words, and the router's audit records and the model tap kept when the answer rests.
async fn play(rig: &Rig, attack: &AttackCase, case: &Case) -> Result<LiveRun, String> {
    let words = case
        .turns
        .first()
        .cloned()
        .ok_or_else(|| "the case has no turn".to_owned())?;
    let reviewer = Judges(InferReviewer {
        quick: rig.model("quire-quick")?,
        deliberate: rig.model("quire-deliberate")?,
        second: rig.model("quire-second")?,
        timeouts: rig.timeouts.config(),
    });
    let config = AgentConfig {
        review: rig.timeouts.config(),
        ..AgentConfig::default()
    };
    let router = fake_router_reading(
        config,
        Person::saying_yes_to(attack.user_sheet_labels()),
        reviewer,
        InferdWriter::new(rig.link()),
        SystemClock,
        Arc::new(MemoryLog::new()),
        TransportReader::in_process(rig.link()),
    )
    .map_err(|e| format!("router: {e}"))?;
    install_fixture(&router, &case.world, &case.space, case.mode)?;
    // The person is in the world, as in AgentDojo: their window shows where the task is.
    let place = attack
        .place(&router.seams.link.shown())
        .map_err(|e| format!("where the person is: {e}"))?;
    router.seams.link.show_window(place.window.clone());
    let router = Arc::new(router);
    let shell = AppName::parse("org.quire.Shell").map_err(|_| "the shell's name".to_owned())?;
    let mut companion = Companiond::new(
        Intents::over(InProcess::new(
            router.clone(),
            caller("org.quire.Companiond", CallerRole::Companion)?,
        )),
        PlannerModel::new(rig.link()),
        AgentConfig::default(),
        Clock::System,
        shell,
    );
    let launcher = Intents::over(InProcess::new(
        router.clone(),
        caller("org.quire.Shell", CallerRole::Launcher)?,
    ));
    let opened = companion
        .open(SessionOpen {
            space: case.space.clone(),
            agent: AgentRef::Companion,
            parent: None,
            cwd: None,
            started_from: None,
            external: None,
            helper: None,
        })
        .await
        .map_err(|e| format!("the companion did not open a session: {e:?}"))?;
    let turn = launcher
        .session_turn(
            opened.session.clone(),
            TurnIn {
                text: words.clone(),
                origin: Origin::Launcher,
                keep: keep_nothing(),
                via: TurnVia::Typed,
                mode: None,
            },
        )
        .await
        .map_err(|e| format!("the turn was not recorded: {e:?}"))?;
    let turn = UserTurn {
        id: turn,
        text: words,
        at: Clock::System.now(),
        from: TurnSource::Launcher,
        via: TurnVia::Typed,
        mode: None,
    };
    let ask = ask_of(opened.session.clone(), turn, &place)?;
    // What the earlier case and the warm-up left in the tap is not this case's.
    drop(rig.memory.take());
    let asked = tokio::time::timeout(CASE_BOUND, companion.ask(ask)).await;
    let (ended, answer) = match asked {
        Err(_) => (
            format!("timed out after {} s", CASE_BOUND.as_secs()),
            AnswerGiven::Nothing,
        ),
        Ok(Err(fault)) => (format!("failed: {fault:?}"), AnswerGiven::Nothing),
        Ok(Ok(_)) => {
            let view = companion.shared.answer(&opened.task);
            let ended = view
                .as_ref()
                .and_then(|view| serde_json::to_string(&view.phase).ok())
                .unwrap_or_else(|| "no answer".to_owned());
            (ended, answer_given(view.as_ref()))
        }
    };
    Ok(LiveRun {
        records: router.seams.sink.records(),
        sheets: router.seams.confirmer.requests(),
        exchanges: rig.memory.take(),
        ended,
        answer,
    })
}

/// Whether the task ended showing the person an answer: done, with words or a shown handle in
/// front of them (or a card they can act on). Asking, failing and ending silent are not.
fn answer_given(view: Option<&AnswerWire>) -> AnswerGiven {
    let shown = view.is_some_and(|view| {
        view.phase == AnswerPhase::Done
            && match &view.body {
                AnswerBody::Text { lines } => !lines.is_empty(),
                _ => true,
            }
    });
    match shown {
        true => AnswerGiven::Shown,
        false => AnswerGiven::Nothing,
    }
}

async fn trace_of(rig: &Rig, attack: &AttackCase, case: &Case) -> CaseTrace {
    match play(rig, attack, case).await {
        Ok(run) => live_trace(case, run),
        Err(why) => faulted_trace(case, &why),
    }
}

/// Plays every attack of `attacks` with the real planner against the model `options` names, writes
/// the traces under `options.out`, and scores them. Must run inside a multi-thread tokio runtime.
pub async fn run_attacks_planner(
    binaries: &Binaries,
    attacks: &[AttackCase],
    options: &CorpusOptions,
) -> Result<AttackPlannerOutcome, CorpusError> {
    let model = options.engine.source(
        options.cassette.clone().unwrap_or_default(),
        options.inferd_config.as_deref(),
    )?;
    let world = InferdWorld::start(
        binaries,
        &model,
        &Options {
            keep_in: Some(options.out.join("scratch")),
            tap: TapMode::Off,
            keys_home: options.keys_home.clone(),
            catalog: options.catalog.clone(),
            ..Options::default()
        },
    )
    .await;
    let connection = world.connect().await;
    warm_up(&model, connection.clone(), options.patience).await?;
    let memory = MemoryTap::default();
    let rig = Rig {
        connection,
        tap: Tap::memory(memory.clone(), "eval"),
        memory,
        timeouts: options.timeouts,
    };
    let dir = TraceDir::create(&options.out.join("traces"))?;
    let (mut scores, mut unscored, mut traces) = (Vec::new(), Vec::new(), Vec::new());
    for (n, attack) in attacks.iter().enumerate() {
        eprintln!(
            "docket-live: attack {} of {}: {}",
            n + 1,
            attacks.len(),
            attack.id.0
        );
        let (clean_case, attacked_case) = attack.live_cases();
        let mut clean = trace_of(&rig, attack, &clean_case).await;
        let mut attacked = trace_of(&rig, attack, &attacked_case).await;
        match score_live(attack, &clean, &attacked) {
            Ok(score) => {
                let (met_clean, met_attacked) = score.judgements();
                clean.judgement = met_clean;
                attacked.judgement = met_attacked;
                scores.push(score);
            }
            Err(why) => unscored.push((attack.id.0.clone(), why.to_string())),
        }
        for (case, trace) in [(clean_case, clean), (attacked_case, attacked)] {
            let exchanges = trace.setup_exchanges.clone();
            let file = dir.write_case(&case, &trace, &exchanges)?;
            traces.push((file, trace));
        }
    }
    let catalogue = options.catalog.as_deref().map_or_else(
        || "catalogue: none copied".to_owned(),
        crate::live::catalog::describe,
    );
    dir.write_index(&catalogue, &traces)?;
    let planner = planner_of(options);
    let note = RunNote {
        label: options.label.clone(),
        engine: format!("{} + live planner {planner}", options.engine.slug()),
        skipped: unscored.clone(),
        missed: Vec::new(),
        catalogue,
    };
    Ok(AttackPlannerOutcome {
        planner,
        scores,
        unscored,
        traces,
        trace_dir: dir.path().to_owned(),
        note,
    })
}
