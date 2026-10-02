//! Reading a model's raw reply into a verdict. Strict by design: the reviewer's words are
//! model output, so anything outside the stage's shape is an error, and an error is never an
//! allow (`tighten` turns it into a confirmation).

use crate::request::{CODES, slug};
use crate::verdict::{ReviewReason, ReviewVerdict};
use docket_core::{ReasonCode, ReasonText, ReviewError, Stage};
use serde_json::{Map, Value};

/// The longest reason a model may write, in characters.
const MAX_REASON: usize = 200;

/// Reads a model's raw reply into a verdict. Quick reads one token (`pass` or `flag`); the
/// others read the record of verdict, code and reason. Unknown words, long reasons and codes
/// that contradict the verdict are `OutOfVocabulary`; anything that is not the stage's shape at
/// all is `Unparseable`. A failure is never an allow.
pub fn parse_verdict(raw: &str, stage: Stage) -> Result<ReviewVerdict, ReviewError> {
    let raw = raw.trim();
    match stage {
        Stage::Quick => quick(raw),
        Stage::Deliberate | Stage::SecondOpinion => record(raw),
    }
}

fn quick(raw: &str) -> Result<ReviewVerdict, ReviewError> {
    match raw {
        "pass" => Ok(ReviewVerdict::Allow),
        "flag" => Ok(ReviewVerdict::Ask {
            why: ReviewReason {
                code: ReasonCode::Uncertain,
                text: ReasonText("flagged by the quick judge".to_owned()),
            },
        }),
        "" => Err(ReviewError::Unparseable),
        _ => Err(ReviewError::OutOfVocabulary),
    }
}

/// The codes that justify an allow; every other code justifies an ask or a refusal.
fn allows(code: ReasonCode) -> bool {
    matches!(
        code,
        ReasonCode::WithinRequest | ReasonCode::CoveredByTaskPolicy | ReasonCode::Routine
    )
}

fn field<'a>(map: &'a Map<String, Value>, name: &str) -> Result<&'a str, ReviewError> {
    match map.get(name) {
        Some(Value::String(text)) => Ok(text),
        Some(_) | None => Err(ReviewError::Unparseable),
    }
}

fn record(raw: &str) -> Result<ReviewVerdict, ReviewError> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(raw) else {
        return Err(ReviewError::Unparseable);
    };
    if map.len() != 3 {
        return Err(ReviewError::Unparseable);
    }
    let (verdict, code, reason) = (
        field(&map, "verdict")?,
        field(&map, "code")?,
        field(&map, "reason")?,
    );
    let code = CODES
        .iter()
        .copied()
        .find(|c| slug(c) == code)
        .ok_or(ReviewError::OutOfVocabulary)?;
    if reason.chars().count() > MAX_REASON {
        return Err(ReviewError::OutOfVocabulary);
    }
    let why = ReviewReason {
        code,
        text: ReasonText(reason.to_owned()),
    };
    match (verdict, allows(code)) {
        ("allow", true) => Ok(ReviewVerdict::Allow),
        ("ask", false) => Ok(ReviewVerdict::Ask { why }),
        ("deny", false) => Ok(ReviewVerdict::Deny { why }),
        // A known verdict whose code contradicts it, or an unknown verdict.
        _ => Err(ReviewError::OutOfVocabulary),
    }
}
