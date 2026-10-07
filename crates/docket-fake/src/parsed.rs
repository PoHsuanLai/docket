//! A reviewer that is a model's raw words read by the real parser. The stages answer with text,
//! as an engine does, and `action_review::parse_verdict` makes of it what the product would, so a
//! case can spoil one stage's reply (a fenced block, an extra key) and watch what the cascade
//! does with the error. A stage with no words set says what the hijacked judge says.

use crate::scripted::Forget;
use action_review::{ReviewRequest, ReviewVerdict, Reviewer, parse_verdict};
use docket_core::{ReviewError, Stage};
use std::sync::Mutex;

/// What an unspoiled stage says: the quick judge passes, the larger stages allow.
fn allowing(stage: Stage) -> &'static str {
    match stage {
        Stage::Quick => "pass",
        Stage::Deliberate | Stage::SecondOpinion => {
            r#"{"verdict":"allow","code":"within_request","reason":"scripted"}"#
        }
    }
}

/// Reviewers whose replies are raw text.
#[derive(Debug, Default)]
pub struct ParsedReviewer {
    spoiled: Mutex<Vec<(Stage, String)>>,
    asked: Mutex<Vec<Stage>>,
}

impl ParsedReviewer {
    /// Sets what `stage` says from now on; the other stages keep what they had.
    pub fn say(&self, stage: Stage, words: &str) {
        let mut spoiled = self.spoiled.lock().unwrap_or_else(|e| e.into_inner());
        spoiled.retain(|(s, _)| *s != stage);
        spoiled.push((stage, words.to_owned()));
    }

    /// Every stage back to allowing.
    pub fn reset(&self) {
        self.spoiled
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// The stages asked since the last `forget`, in order.
    pub fn asked(&self) -> Vec<Stage> {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Forget for ParsedReviewer {
    fn forget(&self) {
        self.asked.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }
}

impl Reviewer for ParsedReviewer {
    async fn review(
        &self,
        stage: Stage,
        _request: &ReviewRequest,
    ) -> Result<ReviewVerdict, ReviewError> {
        self.asked
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(stage);
        let words = {
            let spoiled = self.spoiled.lock().unwrap_or_else(|e| e.into_inner());
            spoiled
                .iter()
                .find(|(s, _)| *s == stage)
                .map_or_else(|| allowing(stage).to_owned(), |(_, w)| w.clone())
        };
        parse_verdict(&words, stage)
    }
}
