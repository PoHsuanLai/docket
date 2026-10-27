//! What the board keeps of finished jobs: their answers, and why jobs stopped.

use companion_core::{Answer, Answers, Hold, KEPT_ANSWERS, MOST_NOTICES, Proposals, ToldLog};
use companion_wire::{ProposalId, RoutineForm, WhenForm};
use docket_core::helper::JobId;
use docket_core::routine::Missed;
use prov::{HelperId, SessionId};

fn answer(n: u64, hold: Hold) -> Answer {
    Answer {
        session: SessionId::parse(&format!("s-{n}")).expect("session"),
        words: format!("words {n}"),
        hold,
    }
}

fn helper() -> HelperId {
    serde_json::from_str("\"h-1\"").expect("helper id")
}

#[test]
fn the_oldest_answers_beyond_the_most_kept_are_dropped_and_their_sessions_returned() {
    let mut answers = Answers::default();
    let most = KEPT_ANSWERS as u64;
    for n in 0..most {
        assert!(answers.keep(JobId(n), answer(n, Hold::Close)).is_empty());
    }
    // An app's session is its process's: dropping its answer closes nothing.
    let evicted = answers.keep(JobId(most), answer(most, Hold::Leave));
    assert_eq!(evicted, [SessionId::parse("s-0").expect("session")]);
    assert_eq!(answers.words(JobId(0)), None);
    assert_eq!(answers.words(JobId(1)), Some("words 1"));
    let evicted = answers.keep(JobId(most + 1), answer(most + 1, Hold::Close));
    assert_eq!(evicted, [SessionId::parse("s-1").expect("session")]);
}

#[test]
fn why_a_job_stopped_is_kept_for_as_many_jobs_as_the_answers() {
    let mut answers = Answers::default();
    for n in 0..=KEPT_ANSWERS as u64 {
        answers.note_why(JobId(n), format!("why {n}"));
    }
    assert_eq!(answers.why(JobId(0)), None);
    assert_eq!(answers.why(JobId(1)), Some("why 1"));
}

#[test]
fn the_sentences_told_are_numbered_and_the_oldest_let_go() {
    let mut told = ToldLog::default();
    for n in 0..=MOST_NOTICES {
        told.tell(&helper(), format!("said {n}"));
    }
    let all = told.since(0);
    assert_eq!(all.len(), MOST_NOTICES);
    assert_eq!(all[0].text, "said 1");
    let last = all.last().expect("last");
    assert_eq!(last.n as usize, MOST_NOTICES + 1);
    assert!(told.since(last.n).is_empty());
}

#[test]
fn proposals_wait_in_order_until_the_person_answers_and_too_many_are_refused() {
    let form = || RoutineForm {
        helper: helper(),
        goal: "Check the mail".to_owned(),
        when: WhenForm::At("daily 08:00".to_owned()),
        missed: Missed::Skip,
        steps: 12,
    };
    let mut proposals = Proposals::default();
    let first = proposals.add(form(), "why".to_owned()).expect("first");
    let second = proposals.add(form(), "why".to_owned()).expect("second");
    assert_eq!(first, ProposalId(1));
    assert_eq!(proposals.list().len(), 2);
    assert_eq!(proposals.take(first).map(|p| p.id), Some(first));
    assert_eq!(proposals.take(first), None);
    assert_eq!(proposals.list()[0].id, second);
    for _ in 0..companion_core::MOST_WAITING {
        let _ = proposals.add(form(), "why".to_owned());
    }
    assert_eq!(proposals.add(form(), "why".to_owned()), None);
}
