//! Reads the rows from `agents.toml` text and the records beside the agents.

use super::model::{
    AgentRow, Agents, Availability, Install, ModelState, Offers, SignInState, Source,
};
use docket_agents::AgentsDir;
use docket_agents::offered::{Offered, Start};
use docket_agents::slug::Slug;
use docket_core::Rewind;

fn text(t: &toml::Table, key: &str) -> Option<String> {
    t.get(key)?
        .as_str()
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

fn source(entry: &toml::Table, dir: &AgentsDir) -> Source {
    let (Some(id), Some(version)) = (text(entry, "registry"), text(entry, "version")) else {
        return Source::Own;
    };
    let installed = Slug::parse(&id)
        .ok()
        .zip(Slug::parse(&version).ok())
        .filter(|(id, version)| dir.versions(id).contains(version))
        .map_or(Install::NotInstalled, |_| Install::Installed);
    Source::Registry {
        id,
        version,
        installed,
    }
}

fn model(chosen: Option<String>, offers: &Offers) -> ModelState {
    let Some(id) = chosen else {
        return ModelState::AgentsOwn;
    };
    let availability = match offers {
        Offers::Seen { models, .. } if !models.is_empty() => models
            .iter()
            .find(|m| m.id == id)
            .map_or(Availability::NoLongerOffered, |m| {
                Availability::Offered(m.name.clone())
            }),
        _ => Availability::NotCheckedYet,
    };
    ModelState::Chosen { id, availability }
}

fn seen(record: Option<Offered>) -> (Offers, SignInState) {
    let Some(r) = record else {
        return (Offers::NotSeenYet, SignInState::NotSeenYet);
    };
    let sign_in = match r.start {
        Start::SignedIn => SignInState::SignedIn,
        Start::NeedsSignIn => SignInState::NeedsSignIn { ways: r.ways },
        Start::Unknown => SignInState::Unknown,
    };
    let offers = Offers::Seen {
        models: r.models,
        in_use: r.in_use,
    };
    (offers, sign_in)
}

/// What `checkpoints` says; unwritten (or not a word we know), the profile decides.
fn rewind(entry: &toml::Table) -> Rewind {
    match text(entry, "checkpoints").as_deref() {
        Some("docket") => Rewind::Docket,
        Some("agent") => Rewind::Agent,
        _ => match text(entry, "profile").as_deref() {
            Some("claude-code") => Rewind::Agent,
            _ => Rewind::Docket,
        },
    }
}

fn row(entry: &toml::Table, dir: &AgentsDir) -> Option<AgentRow> {
    let program = text(entry, "program")?;
    let source = source(entry, dir);
    let record = match &source {
        Source::Registry { id, .. } => Offered::read(dir, id).ok().flatten(),
        Source::Own => None,
    };
    let (offers, sign_in) = seen(record);
    Some(AgentRow {
        label: text(entry, "label").unwrap_or_else(|| program.clone()),
        model: model(text(entry, "model"), &offers),
        way: text(entry, "sign_in"),
        rewind: rewind(entry),
        program,
        source,
        offers,
        sign_in,
    })
}

/// The rows of `agents.toml` (its text), one per listed agent, against the records in `dir`.
/// Lenient: text that is not TOML gives no rows, and an entry with no name is left out. Reads
/// files; starts nothing.
pub fn read_agents(agents_toml: &str, dir: &AgentsDir) -> Agents {
    let Ok(doc) = agents_toml.parse::<toml::Table>() else {
        return Agents::default();
    };
    let rows = doc
        .get("agent")
        .and_then(toml::Value::as_array)
        .map(|list| {
            list.iter()
                .filter_map(toml::Value::as_table)
                .filter_map(|entry| row(entry, dir))
                .collect()
        })
        .unwrap_or_default();
    Agents { rows }
}
