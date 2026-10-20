//! Why on-screen runs ended, joined to their reports. The run daemon says how a run ended in a
//! `Finished` signal that names the report it sent; the report arrives in the inbox. The two
//! arrive in either order, so each waits for the other by the report's message id.

use docket_core::EndWhy;
use prov::{AgentRef, MessageId};
use std::collections::BTreeMap;

/// The report a `Cua1.Run.Finished` signal names and why the run ended, when the signal says both
/// in its enumerated fields (`{"body": {"outcome": .., "report": {"kind": "sent", "v": {"message": ..}}}}`).
pub fn report_end(signal: &str) -> Option<(MessageId, EndWhy)> {
    let body: serde_json::Value = serde_json::from_str(signal).ok()?;
    let why = EndWhy::of_outcome(&body["body"]["outcome"])?;
    let sent = &body["body"]["report"];
    let report = match sent["kind"].as_str()? {
        "sent" => MessageId::parse(sent["v"]["message"].as_str()?).ok()?,
        _ => return None,
    };
    Some((report, why))
}

/// The reasons learned and the reports seen, by report.
#[derive(Debug, Default)]
pub(crate) struct RunEnds {
    why: BTreeMap<MessageId, EndWhy>,
    reported: BTreeMap<MessageId, AgentRef>,
}

impl RunEnds {
    /// A run's report arrived from `agent`: the reason already learned for it, if any.
    pub(crate) fn reported(&mut self, report: MessageId, agent: AgentRef) -> Option<EndWhy> {
        let why = self.why.get(&report).copied();
        self.reported.insert(report, agent);
        why
    }

    /// The daemon said why the run that sent `report` ended: the agent whose report was already
    /// seen, so its roster line can say it now.
    pub(crate) fn learned(&mut self, report: MessageId, why: EndWhy) -> Option<AgentRef> {
        self.why.insert(report.clone(), why);
        self.reported.get(&report).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finished_signal_names_its_report_and_a_reason_only_when_it_has_both() {
        let signal = r#"{"vocab":1,"body":{"outcome":{"kind":"blocked","v":{"kind":"lease_refused"}},"steps":0,"report":{"kind":"sent","v":{"message":"m-48"}}}}"#;
        assert_eq!(
            report_end(signal),
            Some((MessageId::parse("m-48").expect("id"), EndWhy::WindowRefused))
        );
        let done = signal.replace(
            r#"{"kind":"blocked","v":{"kind":"lease_refused"}}"#,
            r#"{"kind":"done"}"#,
        );
        assert_eq!(report_end(&done), None);
        assert_eq!(report_end("not json"), None);
    }

    #[test]
    fn a_reason_and_its_report_meet_whichever_comes_first() {
        let report = MessageId::parse("m-48").expect("id");
        let run = AgentRef::Cua {
            run: prov::RunId::parse("r-1").expect("run"),
        };
        let mut late = RunEnds::default();
        assert_eq!(late.reported(report.clone(), run.clone()), None);
        assert_eq!(
            late.learned(report.clone(), EndWhy::WindowRefused),
            Some(run.clone())
        );
        let mut early = RunEnds::default();
        assert_eq!(early.learned(report.clone(), EndWhy::WindowRefused), None);
        assert_eq!(early.reported(report, run), Some(EndWhy::WindowRefused));
    }
}
