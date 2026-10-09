//! The consent file intentd reads (`$XDG_DATA_HOME/quire/intents/grants.json`): the person's
//! standing "always" for the companion to use an app's data in one Space.

use crate::things::App;
use docket_core::{ActionGrantKey, GrantCaller, GrantTarget};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, GrantId};
use prov::{SpaceId, SpaceScope, UnixSeconds};

/// The grants of the companion for `app`'s classes (mail's are `mail` and `contacts`) in `space`, for
/// interactive use, as the JSON intentd reads.
pub fn standing_json(space: &str, app: App) -> String {
    // An editor's session asks under the editor's own name (the app behind its connection), and
    // opens in the desktop Space: the same consent, given to it.
    let editor = GrantCaller::Editor(prov::ClientName::parse("org.quire.Acp").expect("client"));
    let holders = [
        (GrantCaller::Companion, space),
        (editor.clone(), space),
        (editor, "desktop"),
    ];
    let grants: Vec<docket_core::ActionGrant> = holders
        .into_iter()
        .flat_map(|(caller, space)| {
            app.classes()
                .into_iter()
                .flat_map(|class| [Usage::Interactive, Usage::Background].map(|u| (class, u)))
                .map(move |(class, usage)| (caller.clone(), space, class, usage))
        })
        .enumerate()
        .map(|(n, (caller, space, class, usage))| Grant {
            id: GrantId::parse(&format!("accept-{n}")).expect("grant id"),
            key: ActionGrantKey {
                caller,
                owner: AppName::parse(app.name()).expect("app"),
                target: GrantTarget::App,
                class,
                usage,
                space: SpaceScope::Only(SpaceId::parse(space).expect("space")),
            },
            decision: Decision::Allow,
            scope: GrantScope::Always,
            at: UnixSeconds(0),
        })
        .collect();
    serde_json::to_string_pretty(&grants).expect("grants as json")
}
