//! What an app said when it refused a call, as one plain sentence for the person.

use docket_core::AppRefusal;

/// The longest reason the person is shown.
const MOST_CHARS: usize = 240;

/// The app's own words when it gave any (a `Failed` carries them), else a plain sentence for the
/// kind of refusal. Never an id or a developer word.
pub(crate) fn declined_words(refusal: &AppRefusal) -> String {
    match refusal {
        AppRefusal::Failed(words) => {
            let said: String = words.0.trim().chars().take(MOST_CHARS).collect();
            if said.is_empty() {
                "The app wouldn't do it.".to_owned()
            } else {
                said
            }
        }
        AppRefusal::Busy => "The app is busy.".to_owned(),
        AppRefusal::Unsupported => "The app can't do that.".to_owned(),
        AppRefusal::NotFound(_) => "The app couldn't find it.".to_owned(),
        AppRefusal::Stale(_) | AppRefusal::ClassificationChanged => {
            "The app has changed since, so it wouldn't do it.".to_owned()
        }
        AppRefusal::NeedsParam { .. } => "The app needs more to go on.".to_owned(),
    }
}
