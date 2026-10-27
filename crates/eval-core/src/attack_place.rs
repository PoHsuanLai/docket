//! Where the person is when they say an attack's user task.
//!
//! In AgentDojo the user is inside the environment. A live-planner run needs the same: "summarise
//! this thread" means nothing to a companion that is told the person is nowhere. A case names the
//! place with the optional `here` of the attack file:
//!
//! ```toml
//! here = "t1"            # the item with this world key is open; its siblings are in view
//! here = { list = "t1" } # nothing is open; the list that holds this item is in view
//! ```
//!
//! The key is a key of the attack's world (a mail, a contact or a file). The app and the kind come
//! from what the world's fake apps list (`docket_fake::FakeLink::shown`), so nothing here names
//! an app. The items in view are the open item's siblings: the same app and the same kind.
//!
//! When `here` is absent the place follows from the user's own steps, over the things they act on
//! (the targets of the calls, each counted once):
//!
//! - one thing: that thing is open ("this thread", "this file");
//! - several things: nothing is open, and the list that holds the first of them is in view
//!   ("archive the newsletter, then summarise the invoice thread");
//! - none: the carrier's item is open.
//!
//! Nothing is selected and no field is focused. The scripted runs never ask for a place: their
//! steps name what they act on.

use crate::attack::{AttackCase, Carrier};
use crate::case::{FixtureRef, ScriptedStep};
use docket_core::{
    ContextSnapshot, EntityRef, Here, Selection, TextTarget, Visible, WindowPrivacy,
};
use porter_core::{AppName, Count};
use prov::{EntityKey, Label, Labelled};
use serde::{Deserialize, Serialize};

/// Where an attack file puts the person (the `here` field).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PlaceSpec {
    /// The item with this world key is open.
    Open(String),
    /// Nothing is open; the list that holds the item with this world key is in view.
    List {
        /// The world key of an item of the list.
        list: String,
    },
}

/// A place could not be made from the world's items.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PlaceError {
    /// No item of the world has this key.
    #[error("{0:?} names no item the world shows")]
    Missing(String),
    /// More than one item of the world has this key.
    #[error("{0:?} names more than one item the world shows")]
    Ambiguous(String),
}

/// Where the person is: the app whose window it is, and what that window shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    /// The app the person asked from.
    pub app: AppName,
    /// What its window shows: what a `Context` call answers.
    pub window: ContextSnapshot,
}

/// The item a place is built around, and whether it is open or only its list is in view.
enum Spot<'a> {
    Open(&'a EntityRef),
    Listed(&'a EntityRef),
}

impl Carrier {
    /// The world key of the field's item.
    fn key(&self) -> &str {
        match self {
            Carrier::MailBody(k)
            | Carrier::MailSubject(k)
            | Carrier::ContactName(k)
            | Carrier::FileContent(k) => k,
        }
    }
}

fn find<'a>(shown: &'a [EntityRef], key: &str) -> Result<&'a EntityRef, PlaceError> {
    let mut found = shown.iter().filter(|e| e.id.key.as_str() == key);
    match (found.next(), found.next()) {
        (Some(one), None) => Ok(one),
        (Some(_), Some(_)) => Err(PlaceError::Ambiguous(key.to_owned())),
        (None, _) => Err(PlaceError::Missing(key.to_owned())),
    }
}

fn named<'a>(shown: &'a [EntityRef], thing: &FixtureRef) -> Result<&'a EntityRef, PlaceError> {
    shown
        .iter()
        .find(|e| e.id.kind == thing.kind && e.id.key == thing.key)
        .ok_or_else(|| PlaceError::Missing(thing.key.as_str().to_owned()))
}

impl AttackCase {
    /// What the user's steps act on, each thing once, in the order the steps reach it.
    fn things_acted_on(&self) -> Vec<&FixtureRef> {
        let mut things: Vec<&FixtureRef> = Vec::new();
        for step in &self.user_steps {
            if let ScriptedStep::Call(call) = step {
                for thing in &call.targets {
                    if !things.contains(&thing) {
                        things.push(thing);
                    }
                }
            }
        }
        things
    }

    fn spot<'a>(&self, shown: &'a [EntityRef]) -> Result<Spot<'a>, PlaceError> {
        match &self.here {
            Some(PlaceSpec::Open(key)) => find(shown, key).map(Spot::Open),
            Some(PlaceSpec::List { list }) => find(shown, list).map(Spot::Listed),
            None => match self.things_acted_on().as_slice() {
                [] => find(shown, self.carrier.key()).map(Spot::Open),
                [one] => named(shown, one).map(Spot::Open),
                [first, ..] => named(shown, first).map(Spot::Listed),
            },
        }
    }

    /// Where the person is when they say the user task, over what the world's apps list.
    pub fn place(&self, shown: &[EntityRef]) -> Result<Place, PlaceError> {
        let spot = self.spot(shown)?;
        let (Spot::Open(anchor) | Spot::Listed(anchor)) = spot;
        let app = anchor.id.app.clone();
        let items: Vec<EntityRef> = shown
            .iter()
            .filter(|e| e.id.app == app && e.id.kind == anchor.id.kind)
            .cloned()
            .collect();
        let here = match spot {
            Spot::Open(item) => Here::Entity(item.clone()),
            Spot::Listed(_) => Here::Nowhere,
        };
        let window = ContextSnapshot {
            app: app.clone(),
            window: Labelled {
                value: app.as_str().to_owned(),
                label: Label::trusted_user(),
            },
            here,
            selection: Selection::Nothing,
            visible: Visible {
                kind: Some(anchor.id.kind.clone()),
                total: Count(u32::try_from(items.len()).unwrap_or(u32::MAX)),
                items,
            },
            text_target: TextTarget::None,
            privacy: WindowPrivacy::Normal,
        };
        Ok(Place { app, window })
    }

    /// Why `here` cannot be used against this attack's world, if it cannot.
    pub(crate) fn place_problem(&self) -> Option<&'static str> {
        let key = match self.here.as_ref()? {
            PlaceSpec::Open(key) | PlaceSpec::List { list: key } => key,
        };
        let world = &self.world;
        let holders = world.mail.iter().filter(|m| &m.key == key).count()
            + world.contacts.iter().filter(|c| &c.key == key).count()
            + world.files.iter().filter(|f| &f.key == key).count();
        if EntityKey::parse(key).is_err() {
            Some("here is not a valid key")
        } else if holders == 0 {
            Some("here names no item of the world")
        } else if holders > 1 {
            Some("here names more than one item of the world")
        } else {
            None
        }
    }
}
