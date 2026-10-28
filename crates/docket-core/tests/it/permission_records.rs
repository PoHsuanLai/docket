//! Records of docket-core that carry permission types.

use docket_core::{AuditRecord, NeverOffer, SheetLimits};
use docket_permissions_core::{Ended, GrantEnd};
use prov::{SpaceId, UnixSeconds};

fn at(t: i64) -> UnixSeconds {
    UnixSeconds(t)
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
fn an_old_sheet_reads_as_before_limits() {
    // A sheet written before limits offers no "never" and names no limit.
    assert_eq!(SheetLimits::default().never, NeverOffer::Withheld);
}
