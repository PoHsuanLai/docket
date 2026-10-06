//! A run trace: what one case did, in the order it happened, readable by a person who was not
//! there. The case's words and what it expected, each planned step, the rulings the policy and
//! the reviewers gave with their coded reasons, every sheet that was put to the person, how
//! each step ended, and, in a live run, every request a model was sent and what it said (the
//! tap on the daemons' inferd link). The data is built from what already exists: the router's
//! audit records, the sheet's requests, the runner's endings and the tap's exchanges. Nothing
//! is recorded a second time.
//!
//! Rendering is pure and deterministic: no clock, no ids made up. Every enum is written in its
//! serde slug, so a change to a `Debug` impl cannot change a trace.

use crate::case::{ArgFrom, Case, CaseId, Corpus, Expect, MailField, ScriptedStep};
use crate::runner::{CaseResult, Judgement, StepEnding, judge};
use docket_core::{
    AuditRecord, CallEnd, ConfirmAnswerKind, ConfirmRequest, ExchangeAnswer, ModelExchange,
};
use serde::{Deserialize, Serialize};
use std::fmt::Write as _;

/// How far into the case's records and sheets a step reached, and the exchanges it caused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Cut {
    pub records: usize,
    pub sheets: usize,
    pub exchanges: Vec<ModelExchange>,
}

/// One planned step and everything it caused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StepTrace {
    /// The step as the case scripts it, in one line.
    pub plan: String,
    /// The audit records it wrote, in order.
    pub audit: Vec<AuditRecord>,
    /// The sheets it put to the person (every one is answered with a dismissal).
    pub sheets: Vec<ConfirmRequest>,
    /// The model exchanges it caused.
    pub exchanges: Vec<ModelExchange>,
    /// How it ended.
    pub ending: StepEnding,
}

/// One case, start to judgement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CaseTrace {
    /// The case.
    pub id: CaseId,
    /// Its corpus.
    pub corpus: Corpus,
    /// Why it exists.
    pub why: String,
    /// What it expects.
    pub expect: Expect,
    /// The person's own words.
    pub turns: Vec<String>,
    /// The exchanges and records before the first step (the writer's policy, session setup).
    pub setup_exchanges: Vec<ModelExchange>,
    /// The audit records before the first step.
    pub setup_audit: Vec<AuditRecord>,
    /// The steps.
    pub steps: Vec<StepTrace>,
    /// Whether the expectation held.
    pub judgement: Judgement,
}

/// A closed set in its serde slug, without the quotes of a JSON string.
fn slug<T: Serialize>(value: &T) -> Option<String> {
    let text = serde_json::to_string(value).ok()?;
    Some(
        text.strip_prefix('"')
            .and_then(|t| t.strip_suffix('"'))
            .map_or(text.clone(), str::to_owned),
    )
}

fn arg(from: &ArgFrom) -> String {
    match from {
        ArgFrom::UserTurn { turn } => format!("user_turn#{turn}"),
        ArgFrom::Contact(key) => format!("contact:{key}"),
        ArgFrom::MailBody(msg) => format!("mail_body:{msg}"),
        ArgFrom::MailHeader { msg, field } => format!(
            "mail_header:{msg}.{}",
            match field {
                MailField::From => "from",
                MailField::Subject => "subject",
            }
        ),
        ArgFrom::Literal(text) => format!("literal:{text:?}"),
        ArgFrom::Inbound { step } => format!("inbound:step{step}"),
    }
}

/// One scripted step in one line: the call and where each argument's words came from.
pub fn plan_line(step: &ScriptedStep) -> String {
    match step {
        ScriptedStep::Call(call) => {
            let args: Vec<String> = call
                .args
                .iter()
                .map(|(name, from)| format!("{name}={}", arg(from)))
                .collect();
            let targets: Vec<String> = call
                .targets
                .iter()
                .map(|t| format!("{}:{}", t.kind.as_str(), t.key.as_str()))
                .collect();
            format!(
                "call {} {} targets=[{}] args=[{}]",
                call.app.as_str(),
                call.action.as_str(),
                targets.join(","),
                args.join(" ")
            )
        }
        ScriptedStep::Send(send) => format!(
            "send {} from {} to {} text={}",
            slug(&send.kind).unwrap_or_default(),
            slug(&send.from).unwrap_or_default(),
            slug(&send.to).unwrap_or_default(),
            arg(&send.text)
        ),
    }
}

fn call_end(end: &CallEnd) -> String {
    slug(end).unwrap_or_default()
}

/// One audit record in one line, or none for a record a reader of the trace does not need.
pub fn audit_line(record: &AuditRecord) -> Option<String> {
    match record {
        AuditRecord::Call {
            action,
            effect,
            decided,
            end,
            ..
        } => Some(format!(
            "call   {} {} effect={} decided_by={} end={}",
            action.app.as_str(),
            action.name.as_str(),
            slug(effect).unwrap_or_default(),
            slug(decided).unwrap_or_default(),
            call_end(end)
        )),
        AuditRecord::Review { mark, .. } => Some(format!(
            "review {} verdict={} code={} model={} ({} ms)",
            slug(&mark.stage).unwrap_or_default(),
            slug(&mark.verdict).unwrap_or_default(),
            slug(&mark.code).unwrap_or_default(),
            slug(&mark.model).unwrap_or_default(),
            mark.latency.0
        )),
        AuditRecord::Confirm { answer, input, .. } => Some(format!(
            "confirm answered={} receipt={}",
            answer_slug(answer),
            slug(input).unwrap_or_default()
        )),
        AuditRecord::TaskPolicy { state, change, .. } => Some(format!(
            "policy state={} change={}",
            slug(state).unwrap_or_default(),
            slug(change).unwrap_or_default()
        )),
        AuditRecord::Breaker { trip, .. } => Some(format!(
            "breaker tripped={}",
            slug(trip).unwrap_or_default()
        )),
        AuditRecord::Halt { cause, .. } => {
            Some(format!("halt   cause={}", slug(cause).unwrap_or_default()))
        }
        AuditRecord::Classified { classification, .. } => Some(format!(
            "classified {}",
            slug(classification).unwrap_or_default()
        )),
        AuditRecord::Delegation { .. }
        | AuditRecord::Undo { .. }
        | AuditRecord::TaskStarted { .. }
        | AuditRecord::Message(_)
        | AuditRecord::Episode(_)
        | AuditRecord::Session { .. } => None,
    }
}

fn answer_slug(answer: &ConfirmAnswerKind) -> String {
    slug(answer).unwrap_or_default()
}

fn ending_line(ending: &StepEnding) -> String {
    match ending {
        StepEnding::Ran(effect) => format!("ran (effect {})", slug(effect).unwrap_or_default()),
        StepEnding::Asked(effect) => {
            format!(
                "asked the person (effect {})",
                slug(effect).unwrap_or_default()
            )
        }
        StepEnding::Refused(why) => format!("refused: {}", slug(why).unwrap_or_default()),
        StepEnding::Delivered(integrity) => format!(
            "message delivered, label {}",
            slug(integrity).unwrap_or_default()
        ),
    }
}

impl CaseTrace {
    /// Builds the trace of a finished case from its pieces: `records` and `sheets` are
    /// everything the case wrote (each `Cut` says how far a step got), `setup` the exchanges
    /// before the first step.
    pub(crate) fn assemble(
        case: &Case,
        result: &CaseResult,
        (setup, installed): (Vec<ModelExchange>, usize),
        records: Vec<AuditRecord>,
        sheets: Vec<ConfirmRequest>,
        cuts: Vec<Cut>,
    ) -> Self {
        let first = installed.min(records.len());
        let mut steps = Vec::new();
        let (mut from_record, mut from_sheet) = (first, 0);
        let slice = |all: &[AuditRecord], a: usize, b: usize| all.get(a..b).unwrap_or(&[]).to_vec();
        let setup_audit = slice(&records, 0, first);
        for (cut, ending) in cuts.into_iter().zip(&result.steps) {
            let plan = case
                .planner
                .get(steps.len())
                .map(plan_line)
                .unwrap_or_default();
            let upto = cut.records.min(records.len());
            let sheet_to = cut.sheets.min(sheets.len());
            steps.push(StepTrace {
                plan,
                audit: slice(&records, from_record, upto.max(from_record)),
                sheets: sheets
                    .get(from_sheet..sheet_to.max(from_sheet))
                    .unwrap_or(&[])
                    .to_vec(),
                exchanges: cut.exchanges,
                ending: ending.clone(),
            });
            from_record = upto.max(from_record);
            from_sheet = sheet_to.max(from_sheet);
        }
        Self {
            id: case.id.clone(),
            corpus: case.corpus,
            why: case.why.clone(),
            expect: case.expect.clone(),
            turns: case.turns.clone(),
            setup_exchanges: setup,
            setup_audit,
            steps,
            judgement: judge(&case.expect, result),
        }
    }

    /// The trace as text.
    pub fn render(&self) -> String {
        let mut out = String::new();
        let _ = writeln!(
            out,
            "case {} ({})",
            self.id.0,
            slug(&self.corpus).unwrap_or_default()
        );
        let _ = writeln!(out, "why: {}", self.why);
        let _ = writeln!(out, "expect: {}", slug(&self.expect).unwrap_or_default());
        for (n, turn) in self.turns.iter().enumerate() {
            let _ = writeln!(out, "person #{n}: {turn:?}");
        }
        render_stage(&mut out, "setup", &self.setup_exchanges, &self.setup_audit);
        for (n, step) in self.steps.iter().enumerate() {
            let _ = writeln!(out, "\nstep {n}: {}", step.plan);
            render_stage(&mut out, "", &step.exchanges, &step.audit);
            for sheet in &step.sheets {
                let _ = writeln!(
                    out,
                    "  sheet  {} effect={} offer={} taint={} (dismissed)",
                    sheet.action.as_str(),
                    slug(&sheet.effect).unwrap_or_default(),
                    slug(&sheet.offer).unwrap_or_default(),
                    slug(&sheet.taint).unwrap_or_default()
                );
            }
            let _ = writeln!(out, "  => {}", ending_line(&step.ending));
        }
        let _ = writeln!(
            out,
            "\njudgement: {}",
            match self.judgement {
                Judgement::Met => "met",
                Judgement::Missed => "MISSED",
            }
        );
        out
    }
}

fn render_stage(out: &mut String, label: &str, exchanges: &[ModelExchange], audit: &[AuditRecord]) {
    if !label.is_empty() && exchanges.is_empty() && audit.iter().all(|r| audit_line(r).is_none()) {
        return;
    }
    if !label.is_empty() {
        let _ = writeln!(out, "\n{label}");
    }
    for exchange in exchanges {
        render_exchange(out, exchange);
    }
    for line in audit.iter().filter_map(audit_line) {
        let _ = writeln!(out, "  {line}");
    }
}

fn indent(text: &str) -> String {
    text.lines()
        .map(|l| format!("      | {l}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One model exchange: who asked, what it was sent, how inferd routed it, what came back.
pub fn render_exchange(out: &mut String, e: &ModelExchange) {
    let _ = writeln!(
        out,
        "  model  #{} by {} tier={} class={} shape={} tools={} ({} ms, {} in / {} out tokens)",
        e.n,
        e.by,
        e.tier,
        e.class,
        e.shape,
        e.tools.len(),
        e.took_ms,
        e.input_tokens,
        e.output_tokens
    );
    for note in &e.route {
        let _ = writeln!(out, "    route: {note}");
    }
    for message in &e.messages {
        let _ = writeln!(out, "    sent {}:\n{}", message.role, indent(&message.text));
    }
    match &e.answer {
        ExchangeAnswer::Replied { text, calls, stop } => {
            let _ = writeln!(out, "    said (stop {stop}):\n{}", indent(text));
            for call in calls {
                let _ = writeln!(out, "    call {} {}", call.name, call.args);
            }
        }
        ExchangeAnswer::Refused(why) => {
            let _ = writeln!(out, "    inferd refused: {why}");
        }
        ExchangeAnswer::Failed(why) => {
            let _ = writeln!(out, "    model failed: {why}");
        }
        ExchangeAnswer::Cancelled => {
            let _ = writeln!(out, "    cancelled");
        }
    }
}

/// The index of a run: one line per case, with the file its trace is in.
pub fn render_index(traces: &[(String, &CaseTrace)]) -> String {
    let mut out = String::from("case | corpus | judgement | steps | model exchanges | trace\n");
    for (file, t) in traces {
        let model =
            t.setup_exchanges.len() + t.steps.iter().map(|s| s.exchanges.len()).sum::<usize>();
        let _ = writeln!(
            out,
            "{} | {} | {} | {} | {} | {}",
            t.id.0,
            slug(&t.corpus).unwrap_or_default(),
            match t.judgement {
                Judgement::Met => "met",
                Judgement::Missed => "MISSED",
            },
            t.steps.len(),
            model,
            file
        );
    }
    out
}

/// A file name for a case's trace.
pub fn trace_file(id: &CaseId) -> String {
    format!("{}.trace.txt", id.0)
}
