//! How a job's end is told to the board.

use companion_core::{Answer, Finished, Hold, JobEnding, JobReport, OpenedJob, conclude};
use docket_core::helper::{BoardEvent, JobFault, JobId, ResultHandle};
use prov::{SessionId, TaskId};

fn opened() -> OpenedJob {
    OpenedJob {
        task: TaskId::parse("agent-job-1").expect("task"),
        session: SessionId::parse("s-1").expect("session"),
    }
}

fn finished(ending: JobEnding, opened: Option<OpenedJob>, hold: Hold) -> Finished {
    Finished {
        opened,
        report: JobReport {
            ending,
            calls: Vec::new(),
            owned: Vec::new(),
            why: Some("it said why".to_owned()),
        },
        hold,
    }
}

#[test]
fn a_words_answer_keeps_its_session_open_to_be_handed_on() {
    let job = JobId(1);
    let done = finished(
        JobEnding::Done {
            words: "Two bills.".to_owned(),
        },
        Some(opened()),
        Hold::Close,
    );
    let end = conclude(job, done);
    assert_eq!(
        end.event,
        BoardEvent::Finished {
            job,
            result: ResultHandle {
                task: opened().task,
                session: opened().session,
            },
        }
    );
    assert_eq!(
        end.answer,
        Some(Answer {
            session: opened().session,
            words: "Two bills.".to_owned(),
            hold: Hold::Close,
        })
    );
    assert_eq!(end.close, None);
    assert_eq!(end.why.as_deref(), Some("it said why"));
}

#[test]
fn every_other_end_closes_the_session() {
    let job = JobId(2);
    let failed = conclude(
        job,
        finished(
            JobEnding::Failed(JobFault::NoAnswer),
            Some(opened()),
            Hold::Close,
        ),
    );
    assert_eq!(
        failed.event,
        BoardEvent::Failed {
            job,
            fault: JobFault::NoAnswer
        }
    );
    assert_eq!(failed.close, Some(opened().session));
    assert_eq!(failed.answer, None);
    let stopped = conclude(
        job,
        finished(JobEnding::Stopped, Some(opened()), Hold::Close),
    );
    assert_eq!(stopped.event, BoardEvent::Stopped(job));
    assert_eq!(stopped.close, Some(opened().session));
}

#[test]
fn an_answer_without_a_session_is_a_crash() {
    let job = JobId(3);
    let end = conclude(
        job,
        finished(
            JobEnding::Done {
                words: "lost".to_owned(),
            },
            None,
            Hold::Close,
        ),
    );
    assert_eq!(
        end.event,
        BoardEvent::Failed {
            job,
            fault: JobFault::Crashed
        }
    );
    assert_eq!(end.answer, None);
    assert_eq!(end.close, None);
}

#[test]
fn a_job_that_waits_for_a_sign_in_did_not_fail() {
    let job = JobId(4);
    let end = conclude(
        job,
        finished(JobEnding::NeedsSignIn(Vec::new()), None, Hold::Leave),
    );
    assert_eq!(end.event, BoardEvent::NeedsSignIn(job));
    assert_eq!(end.ways, Some(Vec::new()));
}
