//! The readable transcript of a flow.

use super::{Evidence, Failure, Flow, Kind};
use eval_core::render_exchange;
use std::fmt::Write as _;

/// The readable transcript of a flow: the person's words, the answer's phases, every model
/// exchange, the sheets, what the mail app did, and what failed.
pub fn transcript(flow: Flow, e: &Evidence, failures: &[Failure]) -> String {
    transcript_of(flow.slug(), flow.prompt(), e, failures)
}

/// [`transcript`] for anything that plays the person's words: `name` heads it.
pub fn transcript_of(name: &str, prompt: &str, e: &Evidence, failures: &[Failure]) -> String {
    let mut out = format!("flow {name}\nperson: {prompt:?}\n");
    out.push_str("\nanswer phases:\n");
    for view in e.views() {
        let _ = writeln!(
            out,
            "  {}",
            serde_json::to_string(&view.phase).unwrap_or_default()
        );
    }
    if e.answer.is_err() {
        out.push_str("  (it stopped changing here and never settled)\n");
    }
    out.push_str("\nmodel exchanges, in the order the daemons wrote them:\n");
    for exchange in &e.exchanges {
        render_exchange(&mut out, exchange);
    }
    out.push_str("\nsheets:\n");
    for sheet in &e.sheets {
        let _ = writeln!(
            out,
            "  {} effect={} offer={} taint={}",
            sheet.action.as_str(),
            serde_json::to_string(&sheet.effect).unwrap_or_default(),
            serde_json::to_string(&sheet.offer).unwrap_or_default(),
            serde_json::to_string(&sheet.taint).unwrap_or_default()
        );
    }
    let said = e.said();
    let _ = writeln!(
        out,
        "\nanswer chars: {}\nanswer: {said:?}",
        said.chars().count()
    );
    let _ = writeln!(out, "\napp performed: {:?}", e.performed);
    for m in &e.messages {
        let _ = writeln!(
            out,
            "  held: {} to {:?} threads {:?} ({:?})",
            m.action, m.to, m.threads, m.state
        );
    }
    let _ = writeln!(out, "undo: {:?}", e.undo);
    out.push_str("\nresult:\n");
    if failures.is_empty() {
        out.push_str("  PASS\n");
    }
    for f in failures {
        let _ = writeln!(
            out,
            "  FAIL [{}] {}",
            match f.kind {
                Kind::Safety => "safety",
                Kind::Setup => "setup",
                Kind::Capability => "capability",
            },
            f.what
        );
    }
    out
}

/// What a flow's run came to.
#[derive(Debug)]
pub struct FlowReport {
    /// The flow.
    pub flow: Flow,
    /// What did not hold; empty is a pass.
    pub failures: Vec<Failure>,
    /// The transcript.
    pub transcript: String,
    /// The daemons' standard error, for a failing run.
    pub logs: String,
}
