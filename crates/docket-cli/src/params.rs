//! Parameters as a terminal types them, converted by the manifest's declared type: text (`-`
//! reads stdin), a number, a choice (checked against the manifest), a date, an entity
//! (`<kind>:<key>`, or a handle `#<n>` printed by an earlier command), a file, a link. A value is
//! checked here only far enough to say what is wrong; the router checks it again against the same
//! manifest, and labels it `Untrusted, Source::Cli`.

use crate::exit::Failure;
use crate::resolve::Apps;
use crate::when;
use docket_core::{ActionDecl, Args, CharCount, FileRef, Handle, Lines, ParamNeed};
use docket_core::{ParamName, ParamType, TargetKind, TargetValue, Value};
use porter_core::AppName;
use prov::{Confidentiality, EntityId, EntityKey, EntityKind, Integrity, Label};
use prov::{Labelled, Source};
use std::collections::{BTreeMap, BTreeSet};

/// What standard input holds, if the caller gave it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stdin {
    /// Nothing was piped in.
    Closed,
    /// This was.
    Text(String),
}

/// Standard input, which a command line may read once.
#[derive(Debug)]
pub struct StdinSlot(Option<Stdin>);

impl StdinSlot {
    /// A slot holding `stdin`.
    pub fn new(stdin: Stdin) -> Self {
        Self(Some(stdin))
    }

    fn take(&mut self) -> Result<String, Failure> {
        match self.0.take() {
            Some(Stdin::Text(text)) => Ok(text),
            Some(Stdin::Closed) => Err(Failure::usage(
                "`-` reads standard input, and nothing was piped in",
            )),
            None => Err(Failure::usage("standard input (`-`) can be read once")),
        }
    }
}

/// What a conversion reads besides the word.
#[derive(Debug)]
pub struct Reading<'a> {
    /// The installed apps (an entity's kind names its owner).
    pub apps: &'a Apps,
    /// The app whose action is called.
    pub owner: &'a AppName,
    /// Standard input.
    pub stdin: &'a mut StdinSlot,
}

/// What an argument typed in a terminal is worth: nothing the router should trust. The router
/// derives the same label itself and ignores this one.
fn typed() -> Label {
    Label {
        integrity: Integrity::Untrusted,
        confidentiality: Confidentiality::Public,
        classes: BTreeSet::new(),
        sources: BTreeSet::from([Source::Cli]),
    }
}

fn handle(raw: &str) -> Option<Handle> {
    raw.strip_prefix('#')?.parse().ok().map(Handle)
}

/// `<kind>:<key>`, or a bare key when the kind is known from the declaration.
fn entity(kind: &EntityKind, reading: &Reading<'_>, raw: &str) -> Result<EntityId, Failure> {
    let key = match raw.split_once(':') {
        Some((named, key)) if EntityKind::parse(named).as_ref() == Ok(kind) => key,
        Some((named, _)) if EntityKind::parse(named).is_ok() => {
            return Err(Failure::usage(format!(
                "{raw:?} is a {named}, and a {kind} is wanted"
            )));
        }
        _ => raw,
    };
    let app = reading
        .apps
        .owner_of(kind, reading.owner)
        .ok_or_else(|| Failure::usage(format!("no installed app declares {kind}")))?;
    let key = EntityKey::parse(key)
        .map_err(|_| Failure::usage(format!("{key:?} is not a key of a {kind}")))?;
    Ok(EntityId {
        app,
        kind: kind.clone(),
        key,
    })
}

fn text(
    max: CharCount,
    lines: Lines,
    reading: &mut Reading<'_>,
    raw: &str,
) -> Result<Value, Failure> {
    let body = if raw == "-" {
        let piped = reading.stdin.take()?;
        let piped = piped.strip_suffix('\n').unwrap_or(&piped);
        piped.strip_suffix('\r').unwrap_or(piped).to_owned()
    } else {
        raw.to_owned()
    };
    if body.chars().count() > max.0 as usize {
        return Err(Failure::usage(format!("longer than {} characters", max.0)));
    }
    if lines == Lines::One && body.contains('\n') {
        return Err(Failure::usage("this takes one line"));
    }
    Ok(Value::Text(body))
}

/// One word as a value of `ty`.
pub fn value(ty: &ParamType, reading: &mut Reading<'_>, raw: &str) -> Result<Value, Failure> {
    let usage = |why: String| Failure::usage(why);
    if handle(raw).is_some() && matches!(ty, ParamType::Entities(_)) {
        return Err(usage(
            "a list of things takes things, not a held handle".to_owned(),
        ));
    }
    if let Some(held) = handle(raw)
        && matches!(
            ty,
            ParamType::Text { .. }
                | ParamType::Entity(_)
                | ParamType::File
                | ParamType::Url
                | ParamType::Dynamic(_)
        )
    {
        return Ok(Value::Handle(held));
    }
    match ty {
        ParamType::Text { max, lines } => text(*max, *lines, reading, raw),
        ParamType::Dynamic(_) => text(CharCount(512), Lines::One, reading, raw),
        ParamType::Integer { min, max } => raw
            .parse::<i64>()
            .ok()
            .filter(|n| (*min..=*max).contains(n))
            .map(Value::Integer)
            .ok_or_else(|| usage(format!("expected a whole number from {min} to {max}"))),
        ParamType::Decimal { scale } => when::decimal(raw, *scale)
            .map(Value::Decimal)
            .map_err(|e| usage(e.to_string())),
        ParamType::Date => when::date(raw)
            .map(Value::Date)
            .map_err(|e| usage(e.to_string())),
        ParamType::DateTime => when::instant(raw)
            .map(Value::DateTime)
            .map_err(|e| usage(e.to_string())),
        ParamType::Duration => when::duration(raw)
            .map(Value::Duration)
            .map_err(|e| usage(e.to_string())),
        ParamType::Choice(options) => options
            .iter()
            .find(|o| o.id.as_str() == raw)
            .map(|o| Value::Choice(o.id.clone()))
            .ok_or_else(|| {
                let ids: Vec<&str> = options.iter().map(|o| o.id.as_str()).collect();
                usage(format!("expected one of: {}", ids.join(", ")))
            }),
        ParamType::Entity(kind) => entity(kind, reading, raw).map(Value::Entity),
        ParamType::Entities(kind) => entity(kind, reading, raw).map(|e| Value::Entities(vec![e])),
        ParamType::File => file(raw).map(Value::File),
        ParamType::Url => match raw.split_once("://") {
            Some((scheme, rest)) if !scheme.is_empty() && !rest.is_empty() => {
                Ok(Value::Url(raw.to_owned()))
            }
            _ => Err(usage(
                "expected a link such as https://example.org".to_owned(),
            )),
        },
    }
}

/// A path as the absolute file it names.
pub fn file(raw: &str) -> Result<FileRef, Failure> {
    let path =
        std::path::absolute(raw).map_err(|_| Failure::usage(format!("{raw:?} is not a path")))?;
    FileRef::parse(&path.to_string_lossy())
        .map_err(|_| Failure::usage(format!("{raw:?} is not a usable path")))
}

/// What the action acts on, from the words after its name.
pub fn target(
    decl: &ActionDecl,
    reading: &Reading<'_>,
    words: &[String],
) -> Result<TargetValue, Failure> {
    match (&decl.on, words) {
        (TargetKind::Nothing, []) => Ok(TargetValue::Nothing),
        (TargetKind::Nothing, _) => {
            Err(Failure::usage("this action acts on nothing in particular"))
        }
        (TargetKind::Text, _) => Err(Failure::usage(
            "this action acts on a live text field, which a terminal cannot name",
        )),
        (TargetKind::One(kind), [one]) => {
            entity(kind, reading, one).map(|e| TargetValue::Entities(vec![e]))
        }
        (TargetKind::One(kind), _) => Err(Failure::usage(format!("name exactly one {kind}"))),
        (TargetKind::Many(kind), [_, ..]) => words
            .iter()
            .map(|w| entity(kind, reading, w))
            .collect::<Result<Vec<_>, _>>()
            .map(TargetValue::Entities),
        (TargetKind::Many(kind), []) => Err(Failure::usage(format!("name at least one {kind}"))),
        (TargetKind::Files, [_, ..]) => words
            .iter()
            .map(|w| file(w))
            .collect::<Result<Vec<_>, _>>()
            .map(TargetValue::Files),
        (TargetKind::Files, []) => Err(Failure::usage("name at least one file")),
    }
}

/// The arguments of one call: every `--param` the action declares, converted, and nothing else.
pub fn arguments(
    decl: &ActionDecl,
    reading: &mut Reading<'_>,
    given: &[(String, String)],
) -> Result<Args, Failure> {
    let mut by_name: BTreeMap<ParamName, Vec<&str>> = BTreeMap::new();
    for (name, raw) in given {
        let declared = ParamName::parse(name)
            .ok()
            .filter(|n| decl.params.iter().any(|p| &p.name == n))
            .ok_or_else(|| {
                let known: Vec<String> = decl
                    .params
                    .iter()
                    .map(|p| format!("--{}", p.name.as_str().replace('_', "-")))
                    .collect();
                let takes = if known.is_empty() {
                    "it takes no parameters".to_owned()
                } else {
                    format!("it takes: {}", known.join(" "))
                };
                Failure::usage(format!(
                    "--{} is not a parameter of {} ({takes})",
                    name.replace('_', "-"),
                    decl.name,
                ))
            })?;
        by_name.entry(declared).or_default().push(raw);
    }
    let mut args = Args::new();
    for param in &decl.params {
        let flag = format!("--{}", param.name.as_str().replace('_', "-"));
        let Some(words) = by_name.get(&param.name) else {
            if param.need == ParamNeed::Required {
                return Err(Failure::usage(format!("{flag} is required")));
            }
            continue;
        };
        let converted = match (&param.ty, words.as_slice()) {
            (ParamType::Entities(_), words) => words
                .iter()
                .map(|w| value(&param.ty, reading, w))
                .collect::<Result<Vec<_>, _>>()
                .map(merge)
                .map_err(|e| Failure::usage(format!("{flag}: {}", e.what)))?,
            (_, [one]) => value(&param.ty, reading, one)
                .map_err(|e| Failure::usage(format!("{flag}: {}", e.what)))?,
            (_, _) => return Err(Failure::usage(format!("{flag} was given twice"))),
        };
        args.insert(
            param.name.clone(),
            Labelled {
                value: converted,
                label: typed(),
            },
        );
    }
    Ok(args)
}

/// Several `Entities([x])` values as one list.
fn merge(parts: Vec<Value>) -> Value {
    Value::Entities(
        parts
            .into_iter()
            .flat_map(|v| match v {
                Value::Entities(es) => es,
                _ => vec![],
            })
            .collect(),
    )
}
