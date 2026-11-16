use std::path::{Path, PathBuf};

use codeterm_team_package::{MemoryKind, PackageMemory};

use super::{
    automation_port, deferred, knowledge_dir, mem_kind, AutomationItem, Landed, PayloadPort,
    SkillFolder, TeamMemory,
};
use crate::mem::backend::WriteContext;
use crate::mem::{MemScope, NewMemItem};
use crate::service::plan::plan_skills::{create_plan_skill, list_plan_skills, NewPlanSkill};
use crate::service::plan::PlanError;
use crate::service::ServiceContext;

const ACTOR: &str = "system:team-templates";
const TEAM_MEMORY_TAG: &str = "team-memory";
const MAX_TREE_FILES: usize = 256;
const MAX_TREE_FILE_BYTES: u64 = 2 * 1024 * 1024;
const MEMORY_KINDS: [(MemoryKind, &str); 3] = [
    (MemoryKind::Fact, "fact"),
    (MemoryKind::Rule, "rule"),
    (MemoryKind::Learning, "learning"),
];
/// Chats and session history belong to the people who had them; bake leaves them behind.
const HISTORY_KINDS: [&str; 3] = ["conversation", "daily", "summary"];

pub(super) struct ServicePort<'a> {
    pub svc: &'a ServiceContext,
}

fn source(team_id: &str) -> String {
    format!("team:{team_id}")
}

/// Mem titles are unique per kind, so a Team's copy carries its id.
fn title(team_id: &str, title: &str) -> String {
    format!("{title} ({team_id})")
}

/// Joins a package-relative path under `root`, refusing anything that could escape it.
pub(super) fn under(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let parts: Vec<&str> = relative.split('/').collect();
    if relative.is_empty()
        || relative.starts_with('/')
        || relative.contains('\\')
        || relative.contains(':')
        || parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
    {
        return Err(format!("`{relative}` is not a safe relative path"));
    }
    Ok(parts
        .iter()
        .fold(root.to_path_buf(), |path, part| path.join(part)))
}

pub(super) fn write_under(root: &Path, relative: &str, bytes: &[u8]) -> Result<(), String> {
    let target = under(root, relative)?;
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        codeterm_util::safe_tree::refuse_linked_ancestors(parent)?;
    }
    codeterm_util::atomic_write::write_atomic(&target, bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
pub(super) fn write_under_for_test(root: &Path, relative: &str) -> Result<(), String> {
    write_under(root, relative, b"x")
}

/// Every regular file under `root` by `/`-relative path, sorted; links are skipped, never followed.
pub(super) fn read_tree(root: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut out = Vec::new();
    if !root.is_dir() {
        return Ok(out);
    }
    codeterm_util::safe_tree::refuse_linked_ancestors(root)?;
    let mut pending = vec![(root.to_path_buf(), String::new())];
    while let Some((dir, prefix)) = pending.pop() {
        for entry in std::fs::read_dir(&dir).map_err(|error| error.to_string())? {
            let entry = entry.map_err(|error| error.to_string())?;
            let metadata = std::fs::symlink_metadata(entry.path()).map_err(|e| e.to_string())?;
            if codeterm_util::safe_tree::is_link(&metadata) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            let relative = if prefix.is_empty() {
                name
            } else {
                format!("{prefix}/{name}")
            };
            if metadata.is_dir() {
                pending.push((entry.path(), relative));
            } else if metadata.is_file() {
                if metadata.len() > MAX_TREE_FILE_BYTES {
                    return Err(format!("`{relative}` exceeds {MAX_TREE_FILE_BYTES} bytes"));
                }
                if out.len() >= MAX_TREE_FILES {
                    return Err(format!("more than {MAX_TREE_FILES} files under one folder"));
                }
                out.push((
                    relative,
                    std::fs::read(entry.path()).map_err(|error| error.to_string())?,
                ));
            }
        }
    }
    out.sort_by(|left, right| left.0.cmp(&right.0));
    Ok(out)
}

impl ServicePort<'_> {
    fn plan_id(&self, team_id: &str) -> Option<String> {
        self.svc.plan_store.plan_id_for_team(team_id)
    }
}

/// Registers a skill folder as a plan skill; one already registered under the name stays.
pub(super) fn install_plan_skill(
    svc: &ServiceContext,
    plan_id: &str,
    skill: &SkillFolder,
) -> Result<(), String> {
    let created = match create_plan_skill(
        &svc.plan_store,
        plan_id,
        NewPlanSkill {
            slug: skill.name.clone(),
            title: None,
            area: None,
            check: None,
            rationale: Some("registered from a Team template".to_string()),
        },
    ) {
        Ok(created) => created,
        Err(PlanError::PlanSkillAlreadyExists { .. }) => return Ok(()),
        Err(error) => return Err(error.to_string()),
    };
    let dir = Path::new(&created.skill_md)
        .parent()
        .ok_or("the plan skill has no folder")?
        .to_path_buf();
    for (path, bytes) in &skill.files {
        write_under(&dir, path, bytes)?;
    }
    Ok(())
}

impl PayloadPort for ServicePort<'_> {
    fn write_file(&self, team_id: &str, path: &str, bytes: &[u8]) -> Result<(), String> {
        write_under(&knowledge_dir(team_id), path, bytes)
    }

    fn files(&self, team_id: &str) -> Result<Vec<(String, Vec<u8>)>, String> {
        read_tree(&knowledge_dir(team_id))
    }

    fn seed_memory(&self, team_id: &str, memory: &PackageMemory) -> Result<(), String> {
        let scope = self
            .svc
            .mux
            .read()
            .team_workspace(team_id)
            .map(MemScope::Workspace)
            .unwrap_or(MemScope::Global);
        let mut tags = memory.tags.clone();
        tags.push(TEAM_MEMORY_TAG.to_string());
        let item = NewMemItem {
            kind: mem_kind(memory.kind),
            scope,
            title: title(team_id, &memory.title),
            summary: None,
            area: None,
            tags,
            body: memory.body.clone(),
            source: Some(source(team_id)),
            importance: 0.5,
            source_path: None,
            pinned: false,
        };
        self.svc
            .mem
            .save_with_context(item, false, WriteContext::system(ACTOR))
            .map(|_| ())
            .map_err(|error| error.to_string())
    }

    fn memory(&self, team_id: &str) -> Result<TeamMemory, String> {
        let source = source(team_id);
        let suffix = format!(" ({team_id})");
        let mut memory = TeamMemory::default();
        for (kind, name) in MEMORY_KINDS {
            for item in self.svc.mem.list_by_source_prefix(name, &source)? {
                if item.source.as_deref() != Some(source.as_str()) {
                    continue;
                }
                memory.items.push(PackageMemory {
                    title: item
                        .title
                        .strip_suffix(suffix.as_str())
                        .unwrap_or(&item.title)
                        .to_string(),
                    kind,
                    body: item.body,
                    tags: item
                        .tags
                        .into_iter()
                        .filter(|tag| tag != TEAM_MEMORY_TAG)
                        .collect(),
                });
            }
        }
        for name in HISTORY_KINDS {
            memory.history_excluded += self
                .svc
                .mem
                .list_by_source_prefix(name, &source)?
                .iter()
                .filter(|item| item.source.as_deref() == Some(source.as_str()))
                .count() as u32;
        }
        Ok(memory)
    }

    /// A Team without a plan yet holds the skill until its plan binds.
    fn register_skill(&self, team_id: &str, skill: &SkillFolder) -> Result<Landed, String> {
        match self.plan_id(team_id) {
            Some(plan_id) => install_plan_skill(self.svc, &plan_id, skill).map(|()| Landed::Now),
            None => deferred::hold_skill(team_id, skill).map(|()| Landed::Deferred),
        }
    }

    fn skills(&self, team_id: &str) -> Result<Vec<SkillFolder>, String> {
        let mut skills = deferred::held_skills(team_id)?;
        let Some(plan_id) = self.plan_id(team_id) else {
            return Ok(skills);
        };
        for summary in
            list_plan_skills(&self.svc.plan_store, &plan_id).map_err(|error| error.to_string())?
        {
            if summary.verdict != "keep" {
                continue;
            }
            skills.push(SkillFolder {
                files: read_tree(Path::new(&summary.skill_dir))?,
                name: summary.slug,
                description: summary.purpose,
            });
        }
        Ok(skills)
    }

    /// A Blueprint imports under a live Orchestrator; before that it waits for its readiness.
    fn apply_automation(&self, team_id: &str, item: &AutomationItem) -> Result<Landed, String> {
        let port = automation_port().ok_or("Blueprint automation is not available in this host")?;
        port.check(item)?;
        if deferred::live_orchestrator(self.svc, team_id).is_some() {
            return port.apply(team_id, item).map(|()| Landed::Now);
        }
        deferred::wait_for_orchestrator(self.svc, team_id, item.clone());
        Ok(Landed::Deferred)
    }

    fn automation(&self, team_id: &str) -> Result<Vec<AutomationItem>, String> {
        match automation_port() {
            Some(port) => port.list(team_id),
            None => Ok(Vec::new()),
        }
    }
}
