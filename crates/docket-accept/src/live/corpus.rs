//! A corpus run over a model: every case plays through the router with the real policy writer
//! and the real reviewer cascade (intentd's `InferdWriter` and `action-review`'s `InferReviewer`)
//! asking a real inferd on a private bus, over docket-fake's apps. Each case is judged, traced
//! and tallied; the run ends in a `RunReport` and a directory of traces.

use crate::live::engine::{Engine, EngineError, scripted_cassette};
use crate::live::inferd_world::InferdWorld;
use crate::live::shadow_arm::Arm;
use crate::live::stage::{Asker, asker};
use crate::live::trace_dir::{TraceDir, TraceDirError};
use crate::live::warm::WarmFault;
use crate::live::warm_bus::warm_up;
use crate::world::{Binaries, Options, TapMode};
use action_review::{InferReviewer, ReviewRequest, ReviewVerdict, Reviewer, ShadowLog, Shadowed};
use docket_core::{
    AgentConfig, Millis, ModelExchange, ReviewError, ReviewTimeouts, ShadowMode, Stage,
};
use docket_dbus::tap::{MemoryTap, Tap, Tapped};
use docket_eval::{
    Case, CaseTrace, Harness, Observed, PolicyMode, RunNote, RunReport, ShadowReport, Tallies,
    run_case_traced,
};
use docket_fake::{FakeError, Forget, fake_router_with};
use intentd::{InferdModel, InferdWriter, SystemClock};
use porter_client::{AnyTransport, DbusTransport};
use porter_core::{AccountId, Billing, Locality, ModelId};
use porter_infer::ModelCard;
use std::path::PathBuf;
use std::time::Duration;

/// How long each review stage may take, as a run is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Timeouts {
    /// The shipped values (`agent.review.*`: 300, 3000, 3000 ms). A cloud round trip is slower
    /// than the quick stage's 300 ms, so a cloud run with these asks about everything: that is
    /// what a person on the shipped settings would get.
    Shipped,
    /// The same bound for every stage, for measuring the models rather than the deadline.
    Every(Millis),
}

impl Timeouts {
    fn config(self) -> ReviewTimeouts {
        match self {
            Self::Shipped => AgentConfig::default().review,
            Self::Every(ms) => ReviewTimeouts {
                quick: ms,
                deliberate: ms,
                second: ms,
            },
        }
    }
}

/// What a corpus run is told.
#[derive(Debug, Clone)]
pub struct CorpusOptions {
    /// The report's name.
    pub label: String,
    /// Where the model answers come from.
    pub engine: Engine,
    /// The owner's `inferd.toml` (local and cloud).
    pub inferd_config: Option<PathBuf>,
    /// The cassette a scripted run plays; the hijacked judge's when absent.
    pub cassette: Option<String>,
    /// The review deadlines.
    pub timeouts: Timeouts,
    /// Where traces and the scratch root go.
    pub out: PathBuf,
    /// The accountd binary of a cloud run.
    pub accountd: Option<PathBuf>,
    /// The checked home that holds the cloud account, instead of a key typed per world.
    pub accountd_home: Option<crate::live::accountd_home::AccountdHome>,
    /// The catalogue directory copied into the world (the sibling stoker checkout's by default).
    pub catalog: Option<PathBuf>,
    /// How long each model may take to come up before the run starts (live engines).
    pub patience: Duration,
}

/// What a run produced.
#[derive(Debug)]
pub struct CorpusOutcome {
    /// The metrics.
    pub report: RunReport,
    /// How the run was made and what it missed.
    pub note: RunNote,
    /// Every case's trace, with the file it was written to.
    pub traces: Vec<(String, CaseTrace)>,
    /// Where the trace directory is.
    pub trace_dir: PathBuf,
    /// What the shadow flagger scored; empty unless the run asked for it.
    pub shadow: ShadowReport,
}

/// Why a run could not be made.
#[derive(Debug, thiserror::Error)]
pub enum CorpusError {
    /// The model source is wrong.
    #[error(transparent)]
    Engine(#[from] EngineError),
    /// The fake router could not be built.
    #[error("router: {0}")]
    Router(FakeError),
    /// A model card could not be built from this model name.
    #[error("model card for {0}")]
    ModelCard(&'static str),
    /// A model did not come up before the run started.
    #[error(transparent)]
    Warm(#[from] WarmFault),
    /// A trace could not be written.
    #[error(transparent)]
    Trace(#[from] TraceDirError),
}

/// A reviewer the harness can forget between cases: the real cascade keeps no log.
#[derive(Debug)]
struct Judges<R>(R);

impl<R: Reviewer> Reviewer for Judges<R> {
    async fn review(
        &self,
        stage: Stage,
        request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        self.0.review(stage, request).await
    }
}

impl<R> Forget for Judges<R> {
    fn forget(&self) {}
}

type Link = Tapped<AnyTransport>;

fn card(model: &str) -> Option<ModelCard> {
    Some(ModelCard {
        account: AccountId::parse("local").ok()?,
        model: ModelId::parse(model).ok()?,
        locality: Locality::OnDevice,
        billing: Billing::Free,
        capabilities: vec![],
    })
}

fn stage_ms(exchanges: &[ModelExchange]) -> Vec<(Stage, u32)> {
    exchanges
        .iter()
        .filter_map(|e| match asker(e) {
            Asker::Review(stage) => Some((stage, e.took_ms)),
            Asker::Writer | Asker::Planner | Asker::Other => None,
        })
        .collect()
}

fn all_exchanges(trace: &CaseTrace) -> Vec<ModelExchange> {
    trace
        .setup_exchanges
        .iter()
        .chain(trace.steps.iter().flat_map(|s| s.exchanges.iter()))
        .cloned()
        .collect()
}

/// Runs `cases` against the model `options` names, writes the traces under `options.out`, and
/// returns the report. Must run inside a multi-thread tokio runtime.
pub async fn run_corpus_live(
    binaries: &Binaries,
    cases: &[Case],
    options: &CorpusOptions,
) -> Result<CorpusOutcome, CorpusError> {
    run_corpus_shadowed(binaries, cases, options, ShadowMode::Off).await
}

/// [`run_corpus_live`] with the shadow flagger run beside the quick judge when `shadow` is
/// `Record`. The shadow is recorded only: the run's verdicts are the live reviewer's.
pub async fn run_corpus_shadowed(
    binaries: &Binaries,
    cases: &[Case],
    options: &CorpusOptions,
    shadow: ShadowMode,
) -> Result<CorpusOutcome, CorpusError> {
    let cassette = options
        .cassette
        .clone()
        .unwrap_or_else(|| scripted_cassette(cases));
    let model = options
        .engine
        .source(cassette, options.inferd_config.as_deref())?;
    let world = InferdWorld::start(
        binaries,
        &model,
        &Options {
            keep_in: Some(options.out.join("scratch")),
            tap: TapMode::Off,
            accountd: options.accountd.clone(),
            accountd_home: options.accountd_home.clone(),
            catalog: options.catalog.clone(),
            ..Options::default()
        },
    )
    .await;
    let connection = world.connect().await;
    warm_up(&model, connection.clone(), options.patience).await?;
    let memory = MemoryTap::default();
    let tap = Tap::memory(memory.clone(), "eval");
    let link = || -> Link {
        Tapped::new(
            AnyTransport::Dbus(DbusTransport::over(connection.clone())),
            tap.clone(),
        )
    };
    let model_of = |name: &'static str| {
        card(name)
            .map(|c| InferdModel::new(link(), c))
            .ok_or(CorpusError::ModelCard(name))
    };
    let quick = model_of("quire-quick")?;
    let deliberate = model_of("quire-deliberate")?;
    let second = model_of("quire-second")?;
    let log = ShadowLog::default();
    let reviewer = Shadowed {
        live: Judges(InferReviewer {
            quick,
            deliberate,
            second,
            timeouts: options.timeouts.config(),
        }),
        flagger: Arm::for_engine(options.engine),
        sink: log.clone(),
        mode: shadow,
    };
    let writer: InferdWriter<Link> = InferdWriter::new(link());
    let config = AgentConfig {
        review: options.timeouts.config(),
        ..AgentConfig::default()
    };
    let router =
        fake_router_with(config, reviewer, writer, SystemClock).map_err(CorpusError::Router)?;
    let harness = Harness::over(router);
    let dir = TraceDir::create(&options.out.join("traces"))?;

    let mut tallies = Tallies::default();
    let mut traces = Vec::new();
    let mut missed = Vec::new();
    let mut spent = world.spent_micro_usd();
    for case in cases {
        let (result, trace) = tokio::task::block_in_place(|| {
            run_case_traced(case, &harness, PolicyMode::Written, &mut || memory.take())
        });
        let exchanges = all_exchanges(&trace);
        let now = world.spent_micro_usd();
        let observed = Observed {
            stage_ms: stage_ms(&exchanges),
            case_ms: exchanges.iter().map(|e| e.took_ms).sum(),
            micro_usd: now.saturating_sub(spent),
        };
        spent = now;
        tallies.add(case, &result, &observed);
        tallies.add_shadow(case, &log.take());
        if trace.judgement == docket_eval::Judgement::Missed {
            missed.push(case.id.0.clone());
        }
        let file = dir.write_case(case, &trace, &exchanges)?;
        traces.push((file, trace));
    }
    let catalogue = options.catalog.as_deref().map_or_else(
        || "catalogue: none copied".to_owned(),
        crate::live::catalog::describe,
    );
    dir.write_index(&catalogue, &traces)?;
    let shadow = tallies.shadow().clone();
    let report = RunReport {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        per_corpus: tallies.finish().into_iter().collect(),
    };
    let note = RunNote {
        label: options.label.clone(),
        engine: options.engine.slug().to_owned(),
        skipped: Vec::new(),
        missed,
        catalogue,
    };
    Ok(CorpusOutcome {
        report,
        note,
        traces,
        trace_dir: dir.path().to_owned(),
        shadow,
    })
}
