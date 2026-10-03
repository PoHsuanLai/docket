//! The chips a prompt carries, and the context the person kept: a prompt in a field shows what
//! it will send, and removing a chip drops that context from the turn.

use docket_core::{ContextKeep, ContextSnapshot, Here, Keep, Selection, WindowPrivacy};
use ds_intents::{ChipKind, ContextChip, Removal, Tally};

/// The chips for a context taken at the moment of the summon (before the field turned into a
/// prompt): the query, the results with their count, the selection, the window and the app. A
/// private window shows the app alone.
pub fn chips_of(ctx: &ContextSnapshot) -> Vec<ContextChip> {
    let chip = |kind, label: String, count: Option<u32>, removal| ContextChip {
        kind,
        label,
        count: count.map(Tally),
        removal,
    };
    let app = chip(ChipKind::App, ctx.app.to_string(), None, Removal::Fixed);
    if ctx.privacy == WindowPrivacy::Private {
        return vec![app];
    }
    let mut chips = Vec::new();
    if let Here::View { query: Some(q), .. } = &ctx.here {
        chips.push(chip(
            ChipKind::Query,
            q.value.clone(),
            None,
            Removal::Removable,
        ));
    }
    if ctx.visible.total.0 > 0 {
        chips.push(chip(
            ChipKind::Results,
            "Results".into(),
            Some(ctx.visible.total.0),
            Removal::Removable,
        ));
    }
    match &ctx.selection {
        Selection::Nothing => {}
        Selection::Entities { items, .. } => chips.push(chip(
            ChipKind::Selection,
            "Selection".into(),
            u32::try_from(items.len()).ok(),
            Removal::Removable,
        )),
        Selection::Text(t) => chips.push(chip(
            ChipKind::Text,
            t.value.chars().take(40).collect(),
            None,
            Removal::Removable,
        )),
        Selection::Files(files) => chips.push(chip(
            ChipKind::Selection,
            "Files".into(),
            u32::try_from(files.len()).ok(),
            Removal::Removable,
        )),
    }
    chips.push(chip(
        ChipKind::Window,
        ctx.window.value.clone(),
        None,
        Removal::Removable,
    ));
    chips.push(app);
    chips
}

/// What the person kept: a context is kept when its chip is still there.
pub fn keep_of(chips: &[ContextChip]) -> ContextKeep {
    let kept = |kind| {
        if chips.iter().any(|c| c.kind == kind) {
            Keep::Kept
        } else {
            Keep::Dropped
        }
    };
    ContextKeep {
        query: kept(ChipKind::Query),
        results: kept(ChipKind::Results),
        selection: kept(ChipKind::Selection),
        window: kept(ChipKind::Window),
    }
}
