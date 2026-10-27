//! Grouping by Space: the helpers' records are stored in the Space of their helper, and a
//! routine's watches ask the almanac one Space at a time. Spaces are walls, so nothing here mixes
//! two of them.

use docket_core::helper::{Board, CardRecord, HelperRecord};
use prov::{HelperId, SpaceId};

/// `items` grouped by Space, in the order the Spaces first appear.
pub fn group_by_space<T>(items: impl IntoIterator<Item = (SpaceId, T)>) -> Vec<(SpaceId, Vec<T>)> {
    let mut groups: Vec<(SpaceId, Vec<T>)> = Vec::new();
    for (space, item) in items {
        match groups.iter_mut().find(|(held, _)| *held == space) {
            Some((_, held)) => held.push(item),
            None => groups.push((space, vec![item])),
        }
    }
    groups
}

/// The Space a record belongs to: its helper's. A removed card's helper is no longer on the board
/// after the event, so the board before it is asked too.
pub fn space_of(record: &HelperRecord, before: &Board, after: &Board) -> Option<SpaceId> {
    let card = |id: &HelperId| {
        after
            .card(id)
            .or_else(|| before.card(id))
            .map(|c| c.space.clone())
    };
    match record {
        HelperRecord::Card(CardRecord::Kept(card_kept)) => Some(card_kept.space.clone()),
        HelperRecord::Card(CardRecord::Removed(id)) => card(id),
        HelperRecord::Job(job) => card(&job.owner),
        HelperRecord::Post(post) => card(&post.from),
        HelperRecord::Routine(routine) => card(routine.helper()),
    }
}

/// `records` grouped by the Space of their helper, in the order the Spaces first appear. A record
/// whose helper is nowhere is left out.
pub fn by_space(
    records: impl IntoIterator<Item = HelperRecord>,
    before: &Board,
    after: &Board,
) -> Vec<(SpaceId, Vec<HelperRecord>)> {
    group_by_space(
        records
            .into_iter()
            .filter_map(|record| Some((space_of(&record, before, after)?, record))),
    )
}
