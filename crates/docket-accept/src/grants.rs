//! The consent file intentd reads (`$XDG_DATA_HOME/quire/intents/grants.json`): the person's
//! standing "always" for the companion to use Mail's data in one Space.

use docket_core::{ActionGrantKey, GrantCaller, GrantTarget};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{SpaceId, SpaceScope, UnixSeconds};

/// The grants of the companion for Mail's classes (`mail`, `contacts`) in `space`, for
/// interactive use, as the JSON intentd reads.
pub fn standing_json(space: &str) -> String {
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
            [DataClass::Mail, DataClass::Contacts]
                .into_iter()
                .map(move |class| (caller.clone(), space, class))
        })
        .enumerate()
        .map(|(n, (caller, space, class))| Grant {
            id: GrantId::parse(&format!("accept-{n}")).expect("grant id"),
            key: ActionGrantKey {
                caller,
                owner: AppName::parse("org.quire.Mail").expect("app"),
                target: GrantTarget::App,
                class,
                usage: Usage::Interactive,
                space: SpaceScope::Only(SpaceId::parse(space).expect("space")),
            },
            decision: Decision::Allow,
            scope: GrantScope::Always,
            at: UnixSeconds(0),
        })
        .collect();
    serde_json::to_string_pretty(&grants).expect("grants as json")
}
