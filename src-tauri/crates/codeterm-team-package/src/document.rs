use std::collections::{BTreeMap, HashMap};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::OrderedMap;

/// One ATP package document: the typed form of `team.yaml`, in protocol field order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct TeamDocument {
    pub name: String,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub authors: Vec<PackageAuthor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub links: Option<PackageLinks>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub when: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<PackageLineage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<ProjectNeed>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub requires: Option<PackageRequires>,
    #[serde(default, skip_serializing_if = "OrderedMap::is_empty")]
    #[schemars(with = "BTreeMap<String, PackageInput>")]
    #[ts(as = "HashMap<String, PackageInput>")]
    pub inputs: OrderedMap<PackageInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub permissions: Option<PackagePermissions>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub secrets: Vec<PackageSecret>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<PackageFile>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub memory: Vec<PackageMemory>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<PackageSkill>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub automation: Vec<PackageAutomation>,
    /// Every team role by name, in package order; exactly one is `orchestrator`.
    #[schemars(with = "BTreeMap<String, TeamRole>")]
    #[ts(as = "HashMap<String, TeamRole>")]
    pub roles: OrderedMap<TeamRole>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageAuthor {
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<AuthorRole>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum AuthorRole {
    Author,
    Maintainer,
    Contributor,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageLinks {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issues: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docs: Option<String>,
}

/// The package this one was baked from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageLineage {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum ProjectNeed {
    Required,
    #[serde(rename = "none")]
    NoProject,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageRequires {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harnesses: Vec<String>,
}

/// `key: description`, or the full form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PackageInput {
    Short(String),
    Full(PackageInputSpec),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageInputSpec {
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub choices: Vec<String>,
}

impl PackageInput {
    pub fn description(&self) -> &str {
        match self {
            Self::Short(description) => description,
            Self::Full(spec) => &spec.description,
        }
    }

    /// Inputs are required unless they say otherwise.
    pub fn required(&self) -> bool {
        match self {
            Self::Short(_) => true,
            Self::Full(spec) => spec.required.unwrap_or(true),
        }
    }

    pub fn default_value(&self) -> Option<&str> {
        match self {
            Self::Short(_) => None,
            Self::Full(spec) => spec.default.as_deref(),
        }
    }

    pub fn choices(&self) -> &[String] {
        match self {
            Self::Short(_) => &[],
            Self::Full(spec) => &spec.choices,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackagePermissions {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<PackageNetwork>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo_write: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shell: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser: Option<bool>,
}

/// `true`/`false`, or the hosts the package may reach (wildcards allowed).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PackageNetwork {
    Any(bool),
    Hosts(Vec<String>),
}

/// A secret the package asks for by name; values never travel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageSecret {
    pub name: String,
    pub purpose: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub required: Option<bool>,
}

impl PackageSecret {
    /// Secrets are required unless they say otherwise.
    pub fn is_required(&self) -> bool {
        self.required.unwrap_or(true)
    }
}

/// A package file, inline when `content` is present and otherwise read from the archive or folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageFile {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct PackageMemory {
    pub title: String,
    pub kind: MemoryKind,
    pub body: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Fact,
    Rule,
    Learning,
}

/// A skill folder in the package, or an inline Agent Skill.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PackageSkill {
    Path { path: String },
    Inline(InlineSkill),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct InlineSkill {
    pub name: String,
    pub description: String,
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<SkillFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct SkillFile {
    pub path: String,
    pub content: String,
}

impl PackageSkill {
    /// The inline name; a path skill is named by its SKILL.md frontmatter (`Package::skill_name`).
    pub fn inline_name(&self) -> Option<&str> {
        match self {
            Self::Path { .. } => None,
            Self::Inline(skill) => Some(&skill.name),
        }
    }
}

/// Host-defined automation (Domios: Blueprints), inline or by path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum PackageAutomation {
    Path {
        path: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
    },
    Inline {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        description: Option<String>,
        content: String,
    },
}

/// The spec's reserved built-in manager names; hosts attach authority only to these, and only
/// when the role is kind manager.
pub const BUILTIN_MANAGERS: [&str; 4] = ["orchestrator", "planner", "cartographer", "curator"];

/// The role every team has; it anchors the team and always starts with it.
pub const ORCHESTRATOR: &str = "orchestrator";
pub const PLANNER: &str = "planner";
pub const CARTOGRAPHER: &str = "cartographer";
pub const CURATOR: &str = "curator";

/// Whether `name` is one of the spec's reserved built-in manager names.
pub fn is_builtin_manager(name: &str) -> bool {
    BUILTIN_MANAGERS.contains(&name)
}

/// A role's lifecycle: a manager lives with the team, a worker serves one task or assignment.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "snake_case")]
pub enum RoleKind {
    Manager,
    Worker,
}

impl RoleKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Manager => "manager",
            Self::Worker => "worker",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "manager" => Some(Self::Manager),
            "worker" => Some(Self::Worker),
            _ => None,
        }
    }
}

/// An `x-*` vendor extension value: opaque to ATP, kept verbatim so it survives a round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionValue {
    Scalar { text: String, plain: bool },
    Seq(Vec<ExtensionValue>),
    Map(OrderedMap<ExtensionValue>),
}

/// One team role; `kind` decides its lifecycle and the host decides its authority by name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct TeamRole {
    pub kind: RoleKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub start: Option<RoleStart>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelPreference>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instructions_file: Option<String>,
    #[serde(skip)]
    pub extensions: OrderedMap<ExtensionValue>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum RoleStart {
    Mode(StartMode),
    Schedule { schedule: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    WithTeam,
    OnDemand,
}

/// An ordered `harness:model` preference list, or the resolved model of another role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
#[serde(untagged)]
pub enum ModelPreference {
    List(Vec<String>),
    SameAs { same_as: String },
}

/// The start a role takes when the package omits it: a built-in manager name keeps its
/// documented start; any other manager and every worker starts on demand.
pub fn default_start(kind: RoleKind, name: &str) -> RoleStart {
    match (kind, name) {
        (RoleKind::Manager, ORCHESTRATOR | PLANNER) => RoleStart::Mode(StartMode::WithTeam),
        (RoleKind::Manager, CURATOR) => RoleStart::Schedule {
            schedule: "45m..2h".to_string(),
        },
        _ => RoleStart::Mode(StartMode::OnDemand),
    }
}

impl TeamRole {
    /// A role of `kind` with every optional field empty.
    pub fn new(kind: RoleKind) -> Self {
        Self {
            kind,
            name: None,
            description: None,
            start: None,
            model: None,
            reasoning: None,
            skills: Vec::new(),
            tools: Vec::new(),
            files: Vec::new(),
            instructions: None,
            instructions_file: None,
            extensions: OrderedMap::new(),
        }
    }

    /// The declared start, or the default for this role's name and kind.
    pub fn effective_start(&self, name: &str) -> RoleStart {
        self.start
            .clone()
            .unwrap_or_else(|| default_start(self.kind, name))
    }
}

impl TeamDocument {
    /// `project` defaults to `none`.
    pub fn project_need(&self) -> ProjectNeed {
        self.project.unwrap_or(ProjectNeed::NoProject)
    }

    /// Every declared role as `(name, role)`, in package order.
    pub fn roles(&self) -> impl Iterator<Item = (&str, &TeamRole)> {
        self.roles.iter()
    }

    /// The roles of one kind, in package order.
    pub fn roles_of(&self, kind: RoleKind) -> impl Iterator<Item = (&str, &TeamRole)> {
        self.roles.iter().filter(move |(_, role)| role.kind == kind)
    }

    pub fn role(&self, name: &str) -> Option<&TeamRole> {
        self.roles.get(name)
    }

    /// A manager role by name; a worker of that name is not one.
    pub fn manager(&self, name: &str) -> Option<&TeamRole> {
        self.role(name).filter(|role| role.kind == RoleKind::Manager)
    }
}
