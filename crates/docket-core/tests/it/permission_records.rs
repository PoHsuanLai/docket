//! Records and wire shapes of docket-core that carry permission types.

use docket_core::{AuditRecord, GrantAsk, NeverOffer, SheetLimits};
use docket_permissions_core::{Ended, GrantEnd, Level};
use porter_core::AppName;
use prov::{SpaceId, UnixSeconds};

fn at(t: i64) -> UnixSeconds {
    UnixSeconds(t)
}

fn app(name: &str) -> AppName {
    AppName::parse(name).expect("app")
}

fn space(name: &str) -> SpaceId {
    SpaceId::parse(name).expect("space")
}

#[test]
fn a_dropped_entry_is_audited_in_its_own_words() {
    let record = AuditRecord::GrantsEnded {
        at: at(5),
        ended: Ended {
            why: GrantEnd::SpaceGone(SpaceId::parse("home").expect("space")),
            count: 2,
        },
    };
    let json = serde_json::to_string(&record).expect("json");
    assert_eq!(
        json,
        r#"{"kind":"grants_ended","v":{"at":5,"ended":{"why":{"kind":"space_gone","v":"home"},"count":2}}}"#
    );
    assert_eq!(
        serde_json::from_str::<AuditRecord>(&json).expect("back"),
        record
    );
}

#[test]
fn an_old_grant_request_and_sheet_read_as_before_limits() {
    // A grant request written before limits has no level: it is an always-allow.
    let now = GrantAsk {
        app: app("org.quire.Mail"),
        space: space("work"),
        level: Level::Never,
        run: None,
    };
    let mut old = serde_json::to_value(&now).expect("json");
    old.as_object_mut().expect("object").remove("level");
    let ask: GrantAsk = serde_json::from_value(old).expect("an old request still reads");
    assert_eq!(ask.level, Level::AlwaysAllow);
    // A sheet written before limits offers no "never" and names no limit.
    assert_eq!(SheetLimits::default().never, NeverOffer::Withheld);
}
