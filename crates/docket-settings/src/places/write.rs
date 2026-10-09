//! Writes the places as the `assistant` table of a settings document.

use super::model::{CloudAllowed, ModelChoice, Places, Toggle};

fn row(name: &str, allowed: &str, model: Option<&ModelChoice>) -> toml::Table {
    let mut t = toml::Table::new();
    t.insert("name".to_owned(), name.to_owned().into());
    t.insert("allowed".to_owned(), allowed.to_owned().into());
    if let Some(m) = model {
        t.insert("model".to_owned(), m.0.clone().into());
    }
    t
}

/// The settings text for `places`: the `assistant` table alone, for the app to merge into the file.
pub fn write_places(places: &Places) -> String {
    let own: Vec<toml::Value> = places
        .own
        .iter()
        .map(|o| {
            let allowed = match o.allowed {
                Toggle::On => "on",
                Toggle::Off => "off",
            };
            row(&o.name.0, allowed, o.model.as_ref()).into()
        })
        .collect();
    let cloud: Vec<toml::Value> = places
        .cloud
        .iter()
        .map(|c| {
            let allowed = match c.allowed {
                CloudAllowed::Off => "off",
                CloudAllowed::On => "on",
                CloudAllowed::AskEachTime => "ask",
            };
            let mut t = row(&c.name.0, allowed, c.model.as_ref());
            t.insert("provider".to_owned(), c.provider.clone().into());
            t.into()
        })
        .collect();
    let mut this = toml::Table::new();
    if let Some(m) = &places.this_computer {
        this.insert("model".to_owned(), m.0.clone().into());
    }
    let mut a = toml::Table::new();
    a.insert("this_computer".to_owned(), this.into());
    a.insert("own_computers".to_owned(), own.into());
    a.insert("cloud_accounts".to_owned(), cloud.into());
    let mut doc = toml::Table::new();
    doc.insert("assistant".to_owned(), a.into());
    doc.to_string()
}
