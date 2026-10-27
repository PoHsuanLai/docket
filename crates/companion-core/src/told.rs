//! The sentences the person is told about helpers, kept for the shell to read.

use prov::HelperId;
use std::collections::VecDeque;

/// The most notices kept for the shell to read.
pub const MOST_NOTICES: usize = 20;

/// One sentence the person is told about a helper, once: the shell remembers the number of the
/// last one it showed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Told {
    /// Its number: later sentences have larger numbers.
    pub n: u64,
    /// The helper it is about.
    pub helper: HelperId,
    /// The sentence, in plain words.
    pub text: String,
}

/// The last few sentences told, numbered.
#[derive(Debug, Default)]
pub struct ToldLog {
    said: u64,
    told: VecDeque<Told>,
}

impl ToldLog {
    /// Tells `text` about `helper`; the oldest sentence beyond the most kept is let go.
    pub fn tell(&mut self, helper: &HelperId, text: String) {
        self.said = self.said.saturating_add(1);
        self.told.push_back(Told {
            n: self.said,
            helper: helper.clone(),
            text,
        });
        while self.told.len() > MOST_NOTICES {
            self.told.pop_front();
        }
    }

    /// The sentences told since the one numbered `after`, oldest first.
    pub fn since(&self, after: u64) -> Vec<Told> {
        self.told.iter().filter(|t| t.n > after).cloned().collect()
    }
}
