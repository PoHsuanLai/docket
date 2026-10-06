//! One skill: `skill.toml` and `SKILL.md` parsed and checked as pure functions of their text.

use crate::fault::SkillFault;
use docket_core::{ActionRef, SkillCard, SkillId, SkillText, SkillVersion};
use porter_core::AppName;
use prov::{ActionName, EntityKind, Source};
use serde::Deserialize;

/// The most a body may hold.
pub const BODY_MAX_BYTES: usize = 8 * 1024;
/// The most a catalogue line may hold.
pub const DESCRIPTION_MAX_CHARS: usize = 160;

/// Whether a skill is always preselected: an enum, not a flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Always {
    /// Only when the context matches.
    #[default]
    No,
    /// Every turn (only for a tiny desktop-basics skill).
    Yes,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WhenToml {
    #[serde(default)]
    focused_app: Vec<AppName>,
    #[serde(default)]
    kinds: Vec<String>,
    #[serde(default)]
    always: Always,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SkillToml {
    vocab: u32,
    id: String,
    owner: AppName,
    version: String,
    #[serde(default)]
    uses: Vec<String>,
    #[serde(default)]
    when: WhenToml,
}

/// The preselection hints of a skill (`[when]`).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct When {
    /// Apps whose focus selects it; empty means any.
    pub focused_app: Vec<AppName>,
    /// Entity kinds in the context that select it.
    pub kinds: Vec<EntityKind>,
    /// Whether it is selected every turn.
    pub always: Always,
}

/// Who put the skill there, which sets its label.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// Shipped with the desktop (`$XDG_DATA_DIRS`): the owner app is the source.
    Shipped,
    /// The person's own (`$XDG_DATA_HOME`): the person is the source.
    Own,
}

/// A skill that passed the format rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// Its id.
    pub id: SkillId,
    /// The app that ships it.
    pub owner: AppName,
    /// Its version.
    pub version: SkillVersion,
    /// The actions it teaches.
    pub uses: Vec<ActionRef>,
    /// When it is preselected.
    pub when: When,
    /// The catalogue line.
    pub description: String,
    /// The Markdown body.
    pub body: String,
    /// Where it came from.
    pub origin: Origin,
}

impl Skill {
    /// The catalogue line.
    pub fn card(&self) -> SkillCard {
        SkillCard {
            id: self.id.clone(),
            description: self.description.clone(),
        }
    }

    /// The body as trusted text from its source.
    pub fn text(&self) -> SkillText {
        let source = match self.origin {
            Origin::Shipped => Source::App(self.owner.clone()),
            Origin::Own => Source::User,
        };
        SkillText::new(self.id.clone(), source, self.body.clone())
    }
}

fn use_ref(text: &str) -> Result<ActionRef, SkillFault> {
    let bad = || SkillFault::BadUse(text.to_owned());
    let (app, name) = text.split_once(':').ok_or_else(bad)?;
    Ok(ActionRef {
        app: AppName::parse(app).map_err(|_| bad())?,
        name: ActionName::parse(name).map_err(|_| bad())?,
    })
}

fn kind(text: &str) -> Result<EntityKind, SkillFault> {
    EntityKind::parse(text).map_err(|_| SkillFault::BadKind(text.to_owned()))
}

fn id(text: &str) -> Result<SkillId, SkillFault> {
    SkillId::parse(text).map_err(|_| SkillFault::BadId(text.to_owned()))
}

/// What `skill.toml` says, checked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Facts {
    /// `id`.
    pub id: SkillId,
    /// `owner`.
    pub owner: AppName,
    /// `version`.
    pub version: SkillVersion,
    /// `uses`.
    pub uses: Vec<ActionRef>,
    /// `[when]`.
    pub when: When,
}

/// Parses `skill.toml`.
pub fn parse_facts(text: &str) -> Result<Facts, SkillFault> {
    let raw: SkillToml = toml::from_str(text).map_err(|e| SkillFault::Toml(e.to_string()))?;
    if raw.vocab != 1 {
        return Err(SkillFault::Vocab(raw.vocab));
    }
    Ok(Facts {
        id: id(&raw.id)?,
        owner: raw.owner,
        version: SkillVersion(raw.version),
        uses: raw
            .uses
            .iter()
            .map(|u| use_ref(u))
            .collect::<Result<_, _>>()?,
        when: When {
            focused_app: raw.when.focused_app,
            kinds: raw
                .when
                .kinds
                .iter()
                .map(|k| kind(k))
                .collect::<Result<_, _>>()?,
            always: raw.when.always,
        },
    })
}

/// What `SKILL.md` says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Doc {
    /// Front matter `name`.
    pub name: String,
    /// Front matter `description`.
    pub description: String,
    /// The Markdown after the front matter.
    pub body: String,
}

fn unquote(value: &str) -> &str {
    let v = value.trim();
    for q in ['"', '\''] {
        if let Some(inner) = v.strip_prefix(q).and_then(|r| r.strip_suffix(q)) {
            return inner;
        }
    }
    v
}

/// Parses `SKILL.md`: `---` front matter of `key: value` lines (other keys and indented lines
/// are skipped, as the open Agent Skills layout allows), then the body.
pub fn parse_doc(text: &str) -> Result<Doc, SkillFault> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = text.split_inclusive('\n');
    let mut offset = 0;
    let mut next = || {
        let row = rows.next()?;
        offset += row.len();
        Some(row.trim_end())
    };
    if next() != Some("---") {
        return Err(SkillFault::NoFrontMatter);
    }
    let (mut name, mut description) = (None, None);
    loop {
        let line = next().ok_or(SkillFault::NoFrontMatter)?;
        if line == "---" {
            break;
        }
        if line.starts_with([' ', '\t', '-']) {
            continue;
        }
        if let Some((key, value)) = line.split_once(':') {
            let value = unquote(value);
            match key.trim() {
                "name" if !value.is_empty() => name = Some(value.to_owned()),
                "description" if !value.is_empty() => description = Some(value.to_owned()),
                _ => {}
            }
        }
    }
    Ok(Doc {
        name: name.ok_or(SkillFault::MissingKey("name"))?,
        description: description.ok_or(SkillFault::MissingKey("description"))?,
        body: text.get(offset..).unwrap_or_default().trim().to_owned(),
    })
}

/// Joins the two files of the directory named `dir` into a skill, checking the rules of the
/// format: ids agree, sizes, non-empty body.
pub fn assemble(dir: &str, facts: Facts, doc: Doc, origin: Origin) -> Result<Skill, SkillFault> {
    if facts.id.as_str() != dir || doc.name != dir {
        return Err(SkillFault::IdMismatch {
            dir: dir.to_owned(),
            toml: facts.id.to_string(),
            md: doc.name,
        });
    }
    let chars = doc.description.chars().count();
    if chars > DESCRIPTION_MAX_CHARS {
        return Err(SkillFault::DescriptionTooLong(chars));
    }
    if doc.body.is_empty() {
        return Err(SkillFault::EmptyBody);
    }
    if doc.body.len() > BODY_MAX_BYTES {
        return Err(SkillFault::BodyTooLarge(doc.body.len()));
    }
    Ok(Skill {
        id: facts.id,
        owner: facts.owner,
        version: facts.version,
        uses: facts.uses,
        when: facts.when,
        description: doc.description,
        body: doc.body,
        origin,
    })
}

/// Both files' text to a skill.
pub fn parse(dir: &str, toml: &str, md: &str, origin: Origin) -> Result<Skill, SkillFault> {
    assemble(dir, parse_facts(toml)?, parse_doc(md)?, origin)
}
