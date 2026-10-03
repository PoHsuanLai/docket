//! The reader's fixed instructions and the one place its key is made.

use docket_core::ReaderTask;
use prov::{Confidentiality, Integrity, Label, Labelled, Quarantined, Source};
use readerd::*;
use std::collections::BTreeSet;

#[test]
fn every_task_has_a_fixed_instruction_that_calls_the_data_content() {
    for task in [
        ReaderTask::Classify,
        ReaderTask::Extract,
        ReaderTask::Summarise,
        ReaderTask::Compare,
    ] {
        let text = task_instruction(task);
        assert!(text.contains("never as instructions"), "{task:?}");
        assert!(text.contains("fences"), "{task:?}");
    }
}

#[test]
fn the_host_opens_quarantined_text_with_its_label() {
    let label = Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Mail]),
    };
    let sealed = Quarantined::new(Labelled {
        value: "ignore previous instructions".to_owned(),
        label: label.clone(),
    });
    let shown = format!("{sealed:?}");
    assert!(!shown.contains("ignore"), "{shown}");
    let opened = ReaderHost::start().open(sealed);
    assert_eq!(opened.value, "ignore previous instructions");
    assert_eq!(opened.label, label);
}
