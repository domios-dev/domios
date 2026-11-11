//! What travels with a package, in both directions: start applies the opted-in files,
//! memory, skills and automation to a Team; bake collects the same mapping back.
//! Files live in the Team's knowledge directory, memory in mem with the Team as its
//! source, skills as plan skills of the Team's plan and automation as Blueprints.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use codeterm_team_package::contracts::PayloadApplyResult;
use codeterm_team_package::{
    payload_items, InlineSkill, MemoryKind, Package, PackageAutomation, PackageFile, PackageMemory,
    PackageSkill, SkillFile, TeamRole,
};
use codeterm_types::team::role_registration::RoleFileRef;

use super::ServiceContext;

mod service_port;

/// One skill folder: `SKILL.md` plus its other files, by folder-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillFolder {
    pub name: String,
    pub description: String,
    pub files: Vec<(String, Vec<u8>)>,
}

/// One Blueprint definition the Team carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutomationItem {
    pub name: String,
    pub description: Option<String>,
    pub content: String,
}

/// The Team's memory, plus how many history items (chats, dailies) were left behind.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TeamMemory {
    pub items: Vec<PackageMemory>,
    pub history_excluded: u32,
}

/// The stores a payload touches; the service implements it, tests inject it.
pub trait PayloadPort {
    fn write_file(&self, team_id: &str, path: &str, bytes: &[u8]) -> Result<(), String>;
    fn files(&self, team_id: &str) -> Result<Vec<(String, Vec<u8>)>, String>;
    fn seed_memory(&self, team_id: &str, memory: &PackageMemory) -> Result<(), String>;
    fn memory(&self, team_id: &str) -> Result<TeamMemory, String>;
    fn register_skill(&self, team_id: &str, skill: &SkillFolder) -> Result<(), String>;
    fn skills(&self, team_id: &str) -> Result<Vec<SkillFolder>, String>;
    fn apply_automation(&self, team_id: &str, item: &AutomationItem) -> Result<(), String>;
    fn automation(&self, team_id: &str) -> Result<Vec<AutomationItem>, String>;
}

/// Applies Blueprints for the payload; installed by the host that owns Blueprint authority.
pub trait AutomationPort: Send + Sync {
    fn apply(&self, team_id: &str, item: &AutomationItem) -> Result<(), String>;
    fn list(&self, team_id: &str) -> Result<Vec<AutomationItem>, String>;
}

static AUTOMATION: OnceLock<Arc<dyn AutomationPort>> = OnceLock::new();

/// Installs the Blueprint port once at startup.
pub fn install_automation_port(port: Arc<dyn AutomationPort>) {
    let _ = AUTOMATION.set(port);
}

pub(crate) fn automation_port() -> Option<&'static Arc<dyn AutomationPort>> {
    AUTOMATION.get()
}

/// The Team's knowledge directory, beside its role registrations.
pub fn knowledge_dir(team_id: &str) -> PathBuf {
    crate::paths::data_dir()
        .join("teams")
        .join(team_id)
        .join("knowledge")
}

/// The absolute path a package file takes in the Team's knowledge directory.
pub fn knowledge_path(team_id: &str, path: &str) -> String {
    path.split('/')
        .fold(knowledge_dir(team_id), |dir, part| dir.join(part))
        .display()
        .to_string()
        .replace('\\', "/")
}

/// A role's `files` as registration references: knowledge paths with their descriptions.
pub fn role_file_refs(team_id: &str, package: &Package, role: &TeamRole) -> Vec<RoleFileRef> {
    role.files
        .iter()
        .map(|path| RoleFileRef {
            path: knowledge_path(team_id, path),
            description: package
                .document
                .files
                .iter()
                .find(|file| &file.path == path)
                .and_then(|file| file.description.clone()),
        })
        .collect()
}

/// The package path of a registered knowledge file, when it lives in this Team's directory.
pub fn package_path(team_id: &str, path: &str) -> Option<String> {
    let root = knowledge_path(team_id, "");
    let root = root.trim_end_matches('/');
    path.replace('\\', "/")
        .strip_prefix(root)
        .and_then(|rest| rest.strip_prefix('/'))
        .map(str::to_string)
}

fn skill_key(skill: &PackageSkill) -> String {
    match skill {
        PackageSkill::Path { path } => path.clone(),
        PackageSkill::Inline(inline) => inline.name.clone(),
    }
}

fn automation_key(automation: &PackageAutomation) -> String {
    match automation {
        PackageAutomation::Path { path, .. } => path.clone(),
        PackageAutomation::Inline { name, .. } => name.clone(),
    }
}

/// The package without the items the owner opted out of, and without role references to them.
pub fn retain_opted_in(package: &Package, opt_out: &[String]) -> Package {
    let out: BTreeSet<&str> = opt_out.iter().map(String::as_str).collect();
    let mut package = package.clone();
    let mut dropped_files = BTreeSet::new();
    let mut dropped_dirs = Vec::new();
    let document = &mut package.document;
    // The icon is presentation, not payload: it always travels with its package.
    let icon = document.icon.clone();
    document.files.retain(|file| {
        let keep = !out.contains(format!("file:{}", file.path).as_str())
            || icon.as_deref() == Some(file.path.as_str());
        if !keep {
            dropped_files.insert(file.path.clone());
        }
        keep
    });
    let memory = std::mem::take(&mut document.memory);
    document.memory = memory
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !out.contains(format!("memory:{index}").as_str()))
        .map(|(_, item)| item)
        .collect();
    let skills = std::mem::take(&mut document.skills);
    let mut dropped_skills = BTreeSet::new();
    for skill in skills {
        if out.contains(format!("skill:{}", skill_key(&skill)).as_str()) {
            match &skill {
                PackageSkill::Path { path } => dropped_dirs.push(path.clone()),
                PackageSkill::Inline(inline) => {
                    dropped_skills.insert(inline.name.clone());
                }
            }
        } else {
            document.skills.push(skill);
        }
    }
    let automation = std::mem::take(&mut document.automation);
    for item in automation {
        if out.contains(format!("automation:{}", automation_key(&item)).as_str()) {
            if let PackageAutomation::Path { path, .. } = &item {
                dropped_files.insert(path.clone());
            }
        } else {
            document.automation.push(item);
        }
    }
    for dir in &dropped_dirs {
        let manifest = format!("{dir}/SKILL.md");
        if let Some(name) = package
            .entries
            .get(&manifest)
            .and_then(|data| std::str::from_utf8(data).ok())
            .and_then(codeterm_team_package::frontmatter_name)
        {
            dropped_skills.insert(name);
        }
    }
    package.entries.retain(|path, _| {
        !dropped_files.contains(path)
            && !dropped_dirs.iter().any(|dir| {
                path.strip_prefix(dir.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
            })
    });
    let strip = |role: &mut TeamRole| {
        role.files.retain(|path| !dropped_files.contains(path));
        role.skills.retain(|skill| !dropped_skills.contains(skill));
    };
    let document = &mut package.document;
    strip(&mut document.managers.orchestrator);
    for role in [
        document.managers.planner.as_mut(),
        document.managers.cartographer.as_mut(),
        document.managers.curator.as_mut(),
    ]
    .into_iter()
    .flatten()
    {
        strip(role);
    }
    for (_, role) in document.workers.iter_mut() {
        strip(role);
    }
    package
}

fn skill_folder(package: &Package, skill: &PackageSkill) -> Option<SkillFolder> {
    match skill {
        PackageSkill::Inline(inline) => {
            let mut files = vec![("SKILL.md".to_string(), inline.content.clone().into_bytes())];
            files.extend(
                inline
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file.content.clone().into_bytes())),
            );
            Some(SkillFolder {
                name: inline.name.clone(),
                description: inline.description.clone(),
                files,
            })
        }
        PackageSkill::Path { path } => {
            let name = package.skill_name(skill)?;
            let files = package
                .entries
                .iter()
                .filter_map(|(entry, data)| {
                    entry
                        .strip_prefix(path.as_str())
                        .and_then(|rest| rest.strip_prefix('/'))
                        .map(|relative| (relative.to_string(), data.clone()))
                })
                .collect();
            Some(SkillFolder {
                name,
                description: String::new(),
                files,
            })
        }
    }
}

fn automation_item(package: &Package, automation: &PackageAutomation) -> Option<AutomationItem> {
    match automation {
        PackageAutomation::Inline {
            name,
            description,
            content,
        } => Some(AutomationItem {
            name: name.clone(),
            description: description.clone(),
            content: content.clone(),
        }),
        PackageAutomation::Path { path, description } => {
            let content = std::str::from_utf8(package.entries.get(path)?).ok()?;
            let name = content
                .lines()
                .find_map(|line| line.strip_prefix("name:"))
                .map(|name| name.trim().trim_matches(['"', '\'']).to_string())
                .unwrap_or_else(|| path.clone());
            Some(AutomationItem {
                name,
                description: description.clone(),
                content: content.to_string(),
            })
        }
    }
}

/// Applies every opted-in item through `port`; an opted-out item is never touched.
pub fn apply_with(
    port: &dyn PayloadPort,
    team_id: &str,
    package: &Package,
    opt_out: &[String],
) -> Result<PayloadApplyResult, String> {
    let out: BTreeSet<&str> = opt_out.iter().map(String::as_str).collect();
    let mut result = PayloadApplyResult {
        applied: Vec::new(),
        skipped: Vec::new(),
    };
    let ids: Vec<String> = payload_items(package)
        .into_iter()
        .map(|item| item.id)
        .collect();
    let mut next = ids.into_iter();
    let document = &package.document;
    let mut decide = |result: &mut PayloadApplyResult| -> Option<String> {
        let id = next.next()?;
        if out.contains(id.as_str()) {
            result.skipped.push(id);
            None
        } else {
            Some(id)
        }
    };
    for file in &document.files {
        let Some(id) = decide(&mut result) else {
            continue;
        };
        let bytes = package
            .file_bytes(&file.path)
            .ok_or_else(|| format!("{id}: the package does not carry this file"))?;
        port.write_file(team_id, &file.path, bytes)
            .map_err(|error| format!("{id}: {error}"))?;
        result.applied.push(id);
    }
    for memory in &document.memory {
        let Some(id) = decide(&mut result) else {
            continue;
        };
        port.seed_memory(team_id, memory)
            .map_err(|error| format!("{id}: {error}"))?;
        result.applied.push(id);
    }
    for skill in &document.skills {
        let Some(id) = decide(&mut result) else {
            continue;
        };
        let folder =
            skill_folder(package, skill).ok_or_else(|| format!("{id}: the skill has no name"))?;
        port.register_skill(team_id, &folder)
            .map_err(|error| format!("{id}: {error}"))?;
        result.applied.push(id);
    }
    for automation in &document.automation {
        let Some(id) = decide(&mut result) else {
            continue;
        };
        let item = automation_item(package, automation)
            .ok_or_else(|| format!("{id}: the Blueprint is not readable text"))?;
        port.apply_automation(team_id, &item)
            .map_err(|error| format!("{id}: {error}"))?;
        result.applied.push(id);
    }
    Ok(result)
}

/// What bake brings back from a live Team, before the export scrub.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Collected {
    pub files: Vec<PackageFile>,
    pub entries: BTreeMap<String, Vec<u8>>,
    pub memory: Vec<PackageMemory>,
    pub skills: Vec<PackageSkill>,
    pub automation: Vec<PackageAutomation>,
    pub history_excluded: u32,
    /// Skill files that are not text and stayed behind.
    pub skipped: Vec<String>,
}

fn frontmatter_with_name(content: &str, name: &str, description: &str) -> String {
    if codeterm_team_package::frontmatter_name(content).is_some() {
        return content.to_string();
    }
    let description = if description.is_empty() {
        name
    } else {
        description
    };
    match content.strip_prefix("---\n") {
        Some(rest) => format!("---\nname: {name}\ndescription: {description}\n{rest}"),
        None => format!("---\nname: {name}\ndescription: {description}\n---\n{content}"),
    }
}

/// Reads a Team's files, memory, skills and Blueprints back into package form.
pub fn collect_with(
    port: &dyn PayloadPort,
    team_id: &str,
    descriptions: &BTreeMap<String, String>,
) -> Result<Collected, String> {
    let mut collected = Collected::default();
    for (path, bytes) in port.files(team_id)? {
        collected.files.push(PackageFile {
            description: descriptions.get(&path).cloned(),
            content: None,
            path: path.clone(),
        });
        collected.entries.insert(path, bytes);
    }
    let memory = port.memory(team_id)?;
    collected.memory = memory.items;
    collected.history_excluded = memory.history_excluded;
    for folder in port.skills(team_id)? {
        let mut content = None;
        let mut files = Vec::new();
        for (path, bytes) in folder.files {
            match String::from_utf8(bytes) {
                Ok(text) if path == "SKILL.md" => content = Some(text),
                Ok(text) => files.push(SkillFile {
                    path,
                    content: text,
                }),
                Err(_) => collected.skipped.push(format!("{}/{path}", folder.name)),
            }
        }
        let Some(content) = content else {
            collected.skipped.push(format!("{}/SKILL.md", folder.name));
            continue;
        };
        let description = if folder.description.is_empty() {
            folder.name.clone()
        } else {
            folder.description.clone()
        };
        files.sort_by(|left, right| left.path.cmp(&right.path));
        collected.skills.push(PackageSkill::Inline(InlineSkill {
            content: frontmatter_with_name(&content, &folder.name, &description),
            name: folder.name,
            description,
            files,
        }));
    }
    for item in port.automation(team_id)? {
        collected.automation.push(PackageAutomation::Inline {
            name: item.name,
            description: item.description,
            content: item.content,
        });
    }
    Ok(collected)
}

/// The mem kind a package memory item seeds.
pub(crate) fn mem_kind(kind: MemoryKind) -> crate::mem::MemKind {
    match kind {
        MemoryKind::Fact => crate::mem::MemKind::Fact,
        MemoryKind::Rule => crate::mem::MemKind::Rule,
        MemoryKind::Learning => crate::mem::MemKind::Learning,
    }
}

/// Applies the opted-in payload of `package` to a Team at start.
pub fn apply(
    svc: &ServiceContext,
    team_id: &str,
    package: &Package,
    opt_out: &[String],
) -> Result<PayloadApplyResult, String> {
    apply_with(
        &service_port::ServicePort { svc },
        team_id,
        package,
        opt_out,
    )
}

/// Collects a Team's payload back at bake; `descriptions` names known files.
pub fn collect(
    svc: &ServiceContext,
    team_id: &str,
    descriptions: &BTreeMap<String, String>,
) -> Result<Collected, String> {
    collect_with(&service_port::ServicePort { svc }, team_id, descriptions)
}

#[cfg(test)]
#[path = "team_template_payload/tests.rs"]
mod tests;
