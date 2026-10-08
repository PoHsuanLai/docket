//! A flow played by an external agent, in the same world and judged the same way.

use super::play::{AgentEnd, Played, play};
use super::secret::{Credentials, Redactor};
use super::spec::AcpSpec;
use crate::confirm::{ByEffect, Verdict};
use crate::drive::Launcher;
use crate::live::flows::{
    Evidence, Failure, Flow, Kind, Mode, exchanges_of, judge_in, setup, transcript_of, undo_held,
};
use crate::live::warm_bus::warm_world;
use crate::world::{AcpSetting, Binaries, ModelSource, Options, TapMode, World};
use companion_wire::{AnswerBody, AnswerPhase, AnswerWire, FooterWire};
use docket_core::Reveal;
use docket_session::TurnEnd;
use serde_json::json;
use std::path::PathBuf;
use std::time::Duration;

/// The cassette an agent run plays when the engine is `scripted`: the policy writer allows Mail
/// up to outbound (what the flows ask) and the host's own actions up to reads, and the judges
/// pass. The agent chooses its own calls, so nothing else of a flow's cassette applies.
pub fn agent_cassette() -> String {
    let policy = json!({
        "actions": [
            "org.quire.Mail mail.thread.search",
            "org.quire.Mail mail.thread.current",
            "org.quire.Mail mail.thread.read",
            "org.quire.Mail mail.contact.search",
            "org.quire.Mail mail.message.forward",
            "org.quire.Mail mail.message.send"
        ],
        "apps": [
            {"app": "org.quire.Mail", "up_to": "outbound"},
            {"app": "org.quire.AcpAgent", "up_to": "read"}
        ],
        "kinds": ["mail.thread", "mail.contact"],
        "ceiling": "outbound",
        "max_count": 12,
        "recipients": [],
        "destinations": [],
        "paths": []
    });
    let allow = json!({"verdict": "allow", "code": "within_request", "reason": "scripted"});
    let header = json!({
        "vocab": 1,
        "engine": {"kind": "replay", "build": "docket-live"},
        "model": "scripted",
        "recorded": 0,
        "context": {"loaded": 32768, "trained": 32768},
        "speech": null
    });
    let entry = |anchor: &str, reply: String| {
        json!({
            "when": {"tools": "absent", "contains": [anchor]},
            "reply": {"kind": "text", "v": reply},
            "uses": "always"
        })
    };
    [
        header,
        entry("You write a task policy", policy.to_string()),
        entry("Reply with exactly one word", "\"pass\"".to_owned()),
        entry(
            "Think about whether the action is within the request",
            allow.to_string(),
        ),
        entry("You are an independent second opinion", allow.to_string()),
    ]
    .into_iter()
    .map(|line| format!("{line}\n"))
    .collect()
}

/// What an agent's flow came to.
#[derive(Debug)]
pub struct AcpFlowReport {
    /// The flow.
    pub flow: Flow,
    /// What did not hold; empty is a pass.
    pub failures: Vec<Failure>,
    /// The checks this mode cannot make, each with why.
    pub not_applicable: Vec<String>,
    /// The transcript, with the login scrubbed out.
    pub transcript: String,
}

fn view(end: &AgentEnd, words: &[String]) -> AnswerWire {
    let phase = match end {
        AgentEnd::Turn(TurnEnd::Done) => AnswerPhase::Done,
        AgentEnd::Turn(TurnEnd::Cancelled) => AnswerPhase::Cancelled,
        AgentEnd::Turn(_) | AgentEnd::NotStarted(_) | AgentEnd::Silent => AnswerPhase::Failed,
    };
    AnswerWire {
        task: prov::TaskId::parse("t-agent").unwrap_or_else(|_| unreachable_task()),
        phase,
        body: AnswerBody::Text {
            lines: words.iter().cloned().map(Reveal::Plain).collect(),
        },
        footer: FooterWire {
            served: Vec::new(),
            sources: Vec::new(),
            keep: crate::drive::keep_nothing(),
        },
    }
}

fn unreachable_task() -> prov::TaskId {
    prov::TaskId::parse("t-1").expect("`t-1` is a valid task id")
}

fn evidence(played: &Played, world: &World, undo: crate::live::flows::UndoCheck) -> Evidence {
    let settled = !matches!(played.end, AgentEnd::Silent);
    let views = vec![view(&played.end, &played.words)];
    Evidence {
        answer: if settled { Ok(views) } else { Err(views) },
        sheets: world.sheet.shown(),
        messages: world.mail.messages(),
        performed: world.mail.performed(),
        threads_read: world.mail.threads_read(),
        exchanges: exchanges_of(world),
        undo,
    }
}

fn describe(end: &AgentEnd) -> String {
    match end {
        AgentEnd::Turn(end) => format!("the turn ended {end:?}"),
        AgentEnd::Silent => "the agent went quiet and the turn never ended".to_owned(),
        AgentEnd::NotStarted(why) => format!("the agent did not start: {why}"),
    }
}

/// What the scripted person does with an agent's sheets: "always" for a read-only action, where
/// a planner's first-use grant makes its reads quiet too, and the flow's own answer for the rest
/// (so a refused flow declines the forward itself). The injected-thread flow reads freely and
/// refuses everything else.
fn person_for(flow: Flow) -> ByEffect {
    flow.by_effect().unwrap_or(ByEffect {
        reads: Verdict::AllowAlways,
        rest: flow.verdict(),
    })
}

/// Plays `flow` with the agent `spec` in a fresh world over `model`, with the model tap on and
/// the scratch root kept under `keep_in`. The login of the spec, if any, is staged for the run and
/// removed when this returns (or unwinds); everything written is scrubbed of it.
pub async fn run_flow_acp(
    binaries: &Binaries,
    flow: Flow,
    spec: &AcpSpec,
    model: &ModelSource,
    (keep_in, catalog): (Option<PathBuf>, Option<PathBuf>),
    patience: Duration,
) -> AcpFlowReport {
    let options = Options {
        keep_in,
        tap: TapMode::On,
        accountd: None,
        catalog,
        acp: AcpSetting::Agents,
        focus: flow.focus(),
    };
    let world = World::start_model(binaries, flow.consent(), model, &options).await;
    if let Err(fault) = warm_world(&world, model, patience).await {
        return AcpFlowReport {
            flow,
            failures: vec![setup(&fault)],
            not_applicable: Vec::new(),
            transcript: format!("warm-up failed: {fault}\n"),
        };
    }
    let staged = match &spec.credentials {
        Some(source) => Credentials::stage(source, world.dir.path()).map(Some),
        None => Ok(None),
    };
    let (guard, redactor) = match staged {
        Ok(Some((guard, redactor))) => (Some(guard), redactor),
        Ok(None) => (None, Redactor::none()),
        Err(fault) => {
            return AcpFlowReport {
                flow,
                failures: vec![Failure {
                    kind: Kind::Setup,
                    what: fault.to_string(),
                }],
                not_applicable: Vec::new(),
                transcript: format!("credentials: {fault}\n"),
            };
        }
    };
    world.sheet.will_by_effect(person_for(flow));
    let played = play(
        &world,
        binaries,
        spec,
        guard.as_ref(),
        flow.prompt(),
        patience,
    )
    .await;
    let launcher = Launcher::of(&world).await.patient(patience);
    let undo = undo_held(&launcher, &world).await;
    let found = evidence(&played, &world, undo);
    let mut judged = judge_in(&Mode::Agent(spec.program.clone()), flow, &found);
    if let AgentEnd::NotStarted(why) = &played.end {
        judged.failures.push(Failure {
            kind: Kind::Setup,
            what: format!("the agent did not start: {why}"),
        });
    }
    let mut text = transcript_of(flow.slug(), flow.prompt(), &found, &judged.failures);
    text.push_str(&format!(
        "\nagent {}: {}\n",
        spec.program,
        describe(&played.end)
    ));
    text.push_str("agent said:\n");
    for words in &played.words {
        text.push_str(&format!("  {words:?}\n"));
    }
    text.push_str("calls the host announced:\n");
    for call in &played.calls {
        text.push_str(&format!("  {call}\n"));
    }
    text.push_str("\nnot applicable in this mode:\n");
    if judged.not_applicable.is_empty() {
        text.push_str("  none\n");
    }
    for check in &judged.not_applicable {
        text.push_str(&format!("  {check}\n"));
    }
    // The login goes first from the files, then from the text the caller will write.
    redactor.sweep(world.dir.path());
    drop(guard);
    AcpFlowReport {
        flow,
        failures: judged.failures,
        not_applicable: judged.not_applicable,
        transcript: redactor.scrub(&text),
    }
}
