//! The consent file intentd reads (`$XDG_DATA_HOME/quire/intents/grants.json`): the person's
//! standing "always" for the companion to use Mail's data in one Space.

use docket_core::{ActionGrantKey, GrantCaller, GrantTarget};
use porter_core::consent::{Decision, Grant, GrantScope, Usage};
use porter_core::{AppName, DataClass, GrantId};
use prov::{SpaceId, SpaceScope, UnixSeconds};

/// The grants of the companion for Mail's classes (`mail`, `contacts`) in `space`, for
/// interactive use, as the JSON intentd reads.
pub fn standing_json(space: &str) -> String {
    let grants: Vec<docket_core::ActionGrant> = [DataClass::Mail, DataClass::Contacts]
        .into_iter()
        .enumerate()
        .map(|(n, class)| Grant {
            id: GrantId::parse(&format!("accept-{n}")).expect("grant id"),
            key: ActionGrantKey {
                caller: GrantCaller::Companion,
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
