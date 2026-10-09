//! The actions the planner is offered, from the installed manifests: one tool each, named for
//! the model, with its parameter schema and the declaration its arguments are read by. A hidden
//! action is never a tool. The order is the registry's, so the first sections of the prompt stay
//! byte-stable until the app set changes.

use docket_core::{
    ActionCard, ActionDecl, ActionRef, AgentReach, RelationDecl, TargetKind, ToolSchema,
    ValidManifest, Visibility, tool_schema,
};
use porter_core::AppName;
use porter_infer::{JsonSchemaText, JsonText, ToolDecl, ToolName};
use serde_json::{Value as Json, json};

/// The longest tool name a model is given.
const NAME_LIMIT: usize = 64;

/// One action as a tool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogueTool {
    /// What the model calls it.
    pub name: ToolName,
    /// The app that declares it.
    pub app: AppName,
    /// The declaration: the types its arguments are read by.
    pub decl: ActionDecl,
    /// For a kind's related action, the relations it resolves (from the manifest); empty for
    /// every other action. Such an action is reached through `quire_related`, not as a tool.
    pub related: Vec<RelationDecl>,
}

impl CatalogueTool {
    /// The action.
    pub fn action(&self) -> ActionRef {
        ActionRef {
            app: self.app.clone(),
            name: self.decl.name.clone(),
        }
    }

    /// The card the assembler budgets: label, effect, reach and the argument schema.
    pub fn card(&self) -> ActionCard {
        ActionCard {
            action: self.action(),
            label: self.decl.label.clone(),
            effect: self.decl.effect,
            on: self.decl.on.clone(),
            tool: tool_schema(&self.decl),
            reach: self.decl.reach,
            lasting: self.decl.lasting,
            related: self.related.clone(),
        }
    }

    /// The function declaration the model sees: the parameters as one object, and a `target`
    /// beside them when the action acts on things.
    pub fn declaration(&self) -> Option<ToolDecl> {
        let mut schema = tool_schema(&self.decl).0;
        if let (Some(target), Json::Object(root)) = (target_schema(&self.decl.on), &mut schema)
            && let Some(Json::Object(properties)) = root.get_mut("properties")
        {
            properties.insert(TARGET.to_owned(), target);
        }
        let text = serde_json::to_string(&schema).ok()?;
        Some(ToolDecl {
            name: self.name.clone(),
            description: format!("{} ({:?})", self.decl.label.as_str(), self.decl.effect),
            params: JsonSchemaText(JsonText::parse(&text).ok()?),
        })
    }
}

/// The reserved argument that names what the call acts on.
pub const TARGET: &str = docket_core::TARGET_KEY;

fn entity_or_handle(kind: &str) -> Json {
    json!({
        "anyOf": [
            {
                "type": "object",
                "properties": {
                    "app": { "type": "string" },
                    "kind": { "const": kind },
                    "key": { "type": "string" },
                },
                "required": ["app", "kind", "key"],
                "additionalProperties": false,
            },
            {
                "type": "object",
                "properties": { "handle": { "type": "integer", "minimum": 0 } },
                "required": ["handle"],
                "additionalProperties": false,
            },
        ]
    })
}

/// Says what the target is: the kind, and that a handle you hold names it.
fn described(mut schema: Json, kind: &str) -> Json {
    if let Json::Object(fields) = &mut schema {
        fields.insert(
            "description".to_owned(),
            json!(format!(
                "What this acts on: a {kind}, named as {{\"handle\": n}} with a handle you hold"
            )),
        );
    }
    schema
}

fn target_schema(on: &TargetKind) -> Option<Json> {
    match on {
        TargetKind::Nothing | TargetKind::Text | TargetKind::Files => None,
        TargetKind::One(kind) => Some(described(entity_or_handle(kind.as_str()), kind.as_str())),
        TargetKind::Many(kind) => Some(described(
            json!({
                "type": "array",
                "items": entity_or_handle(kind.as_str()),
                "minItems": 1,
            }),
            kind.as_str(),
        )),
    }
}

/// The tools the planner may call.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Catalogue {
    tools: Vec<CatalogueTool>,
}

impl Catalogue {
    /// The tools of the installed manifests, hidden actions and host-only apps left out.
    pub fn from_manifests(manifests: &[ValidManifest]) -> Self {
        let mut tools: Vec<CatalogueTool> = Vec::new();
        for manifest in manifests
            .iter()
            .map(ValidManifest::manifest)
            .filter(|m| m.visibility == Visibility::Everyone)
        {
            for decl in &manifest.actions {
                if decl.reach == AgentReach::Hidden {
                    continue;
                }
                let name = unique_name(&tools, &manifest.app, decl);
                if let Some(name) = name {
                    tools.push(CatalogueTool {
                        name,
                        app: manifest.app.clone(),
                        decl: decl.clone(),
                        related: manifest
                            .related_kind(&decl.name)
                            .map(|e| e.relations.clone())
                            .unwrap_or_default(),
                    });
                }
            }
        }
        Self { tools }
    }

    /// A catalogue of exactly these tools, in this order: a narrowed copy of another.
    pub fn from_tools(tools: Vec<CatalogueTool>) -> Self {
        Self { tools }
    }

    /// Every tool, in order.
    pub fn tools(&self) -> &[CatalogueTool] {
        &self.tools
    }

    /// The tool the model named.
    pub fn named(&self, name: &str) -> Option<&CatalogueTool> {
        self.tools.iter().find(|t| t.name.as_str() == name)
    }

    /// The tool of an action.
    pub fn of(&self, action: &ActionRef) -> Option<&CatalogueTool> {
        self.tools.iter().find(|t| &t.action() == action)
    }

    /// The cards, most relevant first (the registry's order).
    pub fn cards(&self) -> Vec<ActionCard> {
        self.tools.iter().map(CatalogueTool::card).collect()
    }

    /// The schema of one action's parameters.
    pub fn schema_of(&self, action: &ActionRef) -> Option<ToolSchema> {
        self.of(action).map(|t| tool_schema(&t.decl))
    }
}

/// A name for the model: the app and the action, which carry only `[A-Za-z0-9_.-]`. One that is
/// too long, or taken, is `a<n>` by its place in the catalogue, which is stable while the
/// manifests are.
fn unique_name(tools: &[CatalogueTool], app: &AppName, decl: &ActionDecl) -> Option<ToolName> {
    let long = format!("{}-{}", app.as_str(), decl.name.as_str());
    let taken = |n: &str| tools.iter().any(|t| t.name.as_str() == n);
    let name = if long.len() <= NAME_LIMIT && !taken(&long) {
        long
    } else {
        format!("a{}", tools.len())
    };
    ToolName::parse(&name).ok()
}
