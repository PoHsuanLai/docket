//! `docket-live smoke`.

use super::{CommandFault, say_agent, say_reach};
use crate::Engine;
use crate::acp::{AcpSpec, Redactor, Task, agent_cassette, list_models_acp, run_flow_acp};
use crate::cli::{Agent, SmokeArgs};
use crate::flows::{Flow, Kind, run_flow};
use crate::hostile::run_planner_case;
use docket_accept::catalog;
use docket_accept::keys_home::KeysHome;
use docket_accept::world::{Binaries, ModelSource};
use docket_eval::{PlannerCase, load_planner_cases};
use std::process::ExitCode;
use std::time::Duration;

/// What a smoke run plays: one of the acceptance flows, or one planner case of the hostile-model
/// corpus.
#[derive(Clone)]
enum Play {
    Flow(Flow),
    Hostile(Box<PlannerCase>),
}

impl Play {
    fn name(&self) -> String {
        match self {
            Play::Flow(flow) => flow.slug().to_owned(),
            Play::Hostile(case) => case.id.clone(),
        }
    }

    fn cassette(&self) -> String {
        match self {
            Play::Flow(flow) => flow.cassette().to_owned(),
            Play::Hostile(case) => case.cassette.clone(),
        }
    }
}

/// The flows and planner cases a run is asked for: the named ones, or else every flow, plus the
/// planner cases when the engine is a cassette. A planner case's expectation is written for its
/// own misbehaving replies; a live model says something else, so on a live engine the cases run
/// only when named.
fn plays(args: &SmokeArgs) -> Result<Vec<Play>, CommandFault> {
    let hostile = load_planner_cases(&args.eval_dir.join("hostile-model/planner"))?;
    let all = Flow::ALL
        .into_iter()
        .map(Play::Flow)
        .chain(hostile.iter().cloned().map(|c| Play::Hostile(Box::new(c))));
    if args.flows.is_empty() {
        return Ok(match args.engine {
            Engine::Scripted => all.collect(),
            _ => Flow::ALL.into_iter().map(Play::Flow).collect(),
        });
    }
    args.flows
        .iter()
        .map(|name| {
            all.clone()
                .find(|p| &p.name() == name)
                .ok_or_else(|| format!("no flow or planner case named {name:?}"))
        })
        .collect()
}

/// The flows with an external agent in the companion's place: the same world, the same flows,
/// the same outcome checks. A planner case of the hostile-model corpus scripts the planner's own
/// replies, so it has no agent counterpart and is said to be skipped.
async fn smoke_agent(
    args: SmokeArgs,
    spec: AcpSpec,
    binaries: &Binaries,
) -> Result<ExitCode, CommandFault> {
    let smoke_catalog = args
        .catalog
        .clone()
        .unwrap_or_else(|| catalog::default_source(&args.eval_dir));
    let traces = args.out.join("smoke-acp");
    std::fs::create_dir_all(&traces)?;
    let patience = Duration::from_secs(args.patience_s);
    if spec.task == Task::ListModels {
        let model = args
            .engine
            .source(agent_cassette(), args.inferd_config.as_deref())?;
        let dirs = (Some(args.out.join("scratch")), Some(smoke_catalog));
        print!("{}", list_models_acp(binaries, &spec, &model, dirs).await?);
        return Ok(ExitCode::SUCCESS);
    }
    let redactor = spec
        .credentials
        .as_ref()
        .and_then(|c| std::fs::read(&c.from).ok())
        .map_or_else(Redactor::none, |bytes| Redactor::of(&bytes));
    let flows: Vec<Flow> = if args.flows.is_empty() {
        Flow::ALL.to_vec()
    } else {
        args.flows
            .iter()
            .map(|name| {
                Flow::parse(name).ok_or_else(|| {
                    format!("no flow named {name:?} (a planner case has no agent counterpart)")
                })
            })
            .collect::<Result<_, _>>()?
    };
    println!(
        "N/A hostile-model planner cases: they script the planner's replies, which an agent does not take"
    );
    let mut failed = false;
    for flow in flows {
        let model = args
            .engine
            .source(agent_cassette(), args.inferd_config.as_deref())?;
        let dirs = (Some(args.out.join("scratch")), Some(smoke_catalog.clone()));
        let r = run_flow_acp(binaries, flow, &spec, &model, dirs, patience).await;
        let file = traces.join(format!("{}.trace.txt", flow.slug()));
        let header = format!("{}\n\n", catalog::describe(&smoke_catalog));
        std::fs::write(&file, redactor.scrub(&(header + &r.transcript)))?;
        let verdict = if r.failures.is_empty() {
            "PASS"
        } else {
            "FAIL"
        };
        let name = flow.slug();
        println!(
            "{verdict} {name} (acp:{})  trace: {}",
            spec.program,
            file.display()
        );
        for f in &r.failures {
            let kind = match f.kind {
                Kind::Safety => "safety",
                Kind::Capability => "capability",
                Kind::Setup => "setup",
            };
            println!("     [{kind}] {}", redactor.scrub(&f.what));
        }
        for check in &r.not_applicable {
            println!("     [n/a] {check}");
        }
        failed |= !r.failures.is_empty();
        if r.failures.iter().any(|f| f.kind == Kind::Setup) {
            eprintln!(
                "docket-live: the agent did not come up; the remaining flows are not started"
            );
            break;
        }
    }
    redactor.sweep(&args.out);
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

pub(super) async fn smoke(args: SmokeArgs, binaries: &Binaries) -> Result<ExitCode, CommandFault> {
    say_reach(args.engine);
    say_agent(&args.agent);
    if let Agent::Acp(spec) = args.agent.clone() {
        return smoke_agent(args, *spec, binaries).await;
    }
    let smoke_catalog = args
        .catalog
        .clone()
        .unwrap_or_else(|| catalog::default_source(&args.eval_dir));
    let traces = args.out.join("smoke");
    std::fs::create_dir_all(&traces)?;
    let patience = Duration::from_secs(args.patience_s);
    let dirs = || (Some(args.out.join("scratch")), Some(smoke_catalog.clone()));
    let keys_home = args.keys_home.as_deref().map(KeysHome::open).transpose()?;
    let mut failed = false;
    let plays: Vec<(Play, u32)> = plays(&args)?
        .into_iter()
        .flat_map(|play| {
            let times = if matches!(play, Play::Flow(_)) {
                args.repeat
            } else {
                1
            };
            (1..=times)
                .map(move |run| (play.clone(), run))
                .collect::<Vec<_>>()
        })
        .collect();
    for (play, run) in plays {
        let model: ModelSource = args
            .engine
            .source(play.cassette(), args.inferd_config.as_deref())?;
        let (name, transcript, failures) = match &play {
            Play::Flow(flow) => {
                let r = run_flow(
                    binaries,
                    *flow,
                    &model,
                    dirs().0,
                    dirs().1,
                    keys_home.as_ref(),
                    patience,
                )
                .await;
                (play.name(), r.transcript, r.failures)
            }
            Play::Hostile(case) => {
                let r =
                    run_planner_case(binaries, case, &model, dirs(), keys_home.as_ref(), patience)
                        .await;
                (play.name(), r.transcript, r.failures)
            }
        };
        let name = match (&play, args.repeat) {
            (Play::Flow(_), n) if n > 1 => format!("{name}.run{run}of{n}"),
            _ => name,
        };
        let file = traces.join(format!("{name}.trace.txt"));
        let header = format!("{}\n\n", catalog::describe(&smoke_catalog));
        std::fs::write(&file, header + &transcript)?;
        let verdict = if failures.is_empty() { "PASS" } else { "FAIL" };
        println!("{verdict} {name}  trace: {}", file.display());
        for f in &failures {
            let kind = match f.kind {
                Kind::Safety => "safety",
                Kind::Capability => "capability",
                Kind::Setup => "setup",
            };
            println!("     [{kind}] {}", f.what);
        }
        failed |= !failures.is_empty();
        if failures.iter().any(|f| f.kind == Kind::Setup) {
            eprintln!("docket-live: a model did not come up; the remaining flows are not started");
            break;
        }
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}
