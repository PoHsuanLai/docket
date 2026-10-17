//! The helpers' records reach memory as `Area { Helpers }` events of their own tags, in the Space
//! of the helper.

use almanac_core::{AreaTag, EventBody, JsonText};
use docket_core::AuditRecord;
use docket_core::helper::HelperRecordKind;
use docket_memory::{kind_tag_of, record_of};
use prov::{SpaceId, UnixSeconds};

#[test]
fn a_helper_record_is_stored_under_helpers_with_the_tag_of_its_kind_in_its_own_space() {
    let work = SpaceId::parse("work").expect("space");
    for kind in HelperRecordKind::ALL {
        let record = AuditRecord::Helper {
            at: UnixSeconds(5),
            space: work.clone(),
            kind,
            json: JsonText::parse("{}").expect("json"),
        };
        assert_eq!(
            kind_tag_of(&record).map(|tag| tag.as_str().to_owned()),
            Some(kind.slug().to_owned())
        );
        // The Space the record names wins over the one the caller guessed.
        let made = record_of(&record, &SpaceId::desktop());
        assert_eq!(made.space, work);
        assert_eq!(made.occurred, UnixSeconds(5));
        match made.body {
            EventBody::Area(payload) => {
                assert_eq!(payload.area, AreaTag::Helpers);
                assert_eq!(payload.kind.as_str(), kind.slug());
            }
            other => panic!("an area payload, not {other:?}"),
        }
    }
}
