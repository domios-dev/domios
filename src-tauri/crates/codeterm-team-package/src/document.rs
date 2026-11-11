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
    pub managers: TeamManagers,
    #[serde(default, skip_serializing_if = "OrderedMap::is_empty")]
    #[schemars(with = "BTreeMap<String, TeamRole>")]
    #[ts(as = "HashMap<String, TeamRole>")]
    pub workers: OrderedMap<TeamRole>,
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

/// Fixed manager slots with host-known duties.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema, TS)]
pub struct TeamManagers {
    pub orchestrator: TeamRole,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub planner: Option<TeamRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cartographer: Option<TeamRole>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub curator: Option<TeamRole>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema, TS,
)]
#[serde(rename_all = "snake_case")]
pub enum ManagerSlot {
    Orchestrator,
    Planner,
    Cartographer,
    Curator,
}

impl ManagerSlot {
    pub const ALL: [Self; 4] = [
        Self::Orchestrator,
        Self::Planner,
        Self::Cartographer,
        Self::Curator,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Orchestrator => "orchestrator",
            Self::Planner => "planner",
            Self::Cartographer => "cartographer",
            Self::Curator => "curator",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|slot| slot.as_str() == value)
    }
}

impl TeamManagers {
    pub fn slot(&self, slot: ManagerSlot) -> Option<&TeamRole> {
        match slot {
            ManagerSlot::Orchestrator => Some(&self.orchestrator),
            ManagerSlot::Planner => self.planner.as_ref(),
            ManagerSlot::Cartographer => self.cartographer.as_ref(),
            ManagerSlot::Curator => self.curator.as_ref(),
        }
    }

    pub fn iter(&self) -> impl Iterator<Item = (ManagerSlot, &TeamRole)> {
        ManagerSlot::ALL
            .into_iter()
            .filter_map(|slot| self.slot(slot).map(|role| (slot, role)))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema, TS)]
#[serde(rename_all = "snake_case")]
pub enum TeamRoleGroup {
    Manager,
    Worker,
}

/// An `x-*` vendor extension value: opaque to ATP, kept verbatim so it survives a round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtensionValue {
    Scalar { text: String, plain: bool },
    Seq(Vec<ExtensionValue>),
    Map(OrderedMap<ExtensionValue>),
}

/// The same field set serves manager slots and workers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema, TS)]
pub struct TeamRole {
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

impl ManagerSlot {
    /// The start a slot takes when the package omits it.
    pub fn default_start(self) -> RoleStart {
        match self {
            Self::Orchestrator | Self::Planner => RoleStart::Mode(StartMode::WithTeam),
            Self::Cartographer => RoleStart::Mode(StartMode::OnDemand),
            Self::Curator => RoleStart::Schedule {
                schedule: "45m..2h".to_string(),
            },
        }
    }
}

impl TeamRole {
    /// The declared start, or the slot default; workers start on demand.
    pub fn effective_start(&self, slot: Option<ManagerSlot>) -> RoleStart {
        self.start.clone().unwrap_or_else(|| match slot {
            Some(slot) => slot.default_start(),
            None => RoleStart::Mode(StartMode::OnDemand),
        })
    }
}

impl TeamDocument {
    /// `project` defaults to `none`.
    pub fn project_need(&self) -> ProjectNeed {
        self.project.unwrap_or(ProjectNeed::NoProject)
    }

    /// Every declared role as `(group, role name, role)`; manager names are their slot ids.
    pub fn roles(&self) -> Vec<(TeamRoleGroup, &str, &TeamRole)> {
        let managers = self
            .managers
            .iter()
            .map(|(slot, role)| (TeamRoleGroup::Manager, slot.as_str(), role));
        let workers = self
            .workers
            .iter()
            .map(|(name, role)| (TeamRoleGroup::Worker, name, role));
        managers.chain(workers).collect()
    }

    pub fn role(&self, name: &str) -> Option<&TeamRole> {
        match ManagerSlot::parse(name) {
            Some(slot) => self.managers.slot(slot),
            None => self.workers.get(name),
        }
    }
}
