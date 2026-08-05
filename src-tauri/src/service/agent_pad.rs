use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

const PAD_SUBDIR: &str = "runtime/agent-pads";
const TOMBSTONE_PREFIX: &str = ".revoked-";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AgentPadRole {
    General,
    Orchestrator,
    Planner,
    Worker,
}

/// The one mapping from a tab's role kind to its pad identity. Restore and
/// spawn both go through here so a restarted tab reopens the SAME pad.
pub fn pad_role_for_tab(tab: &crate::multiplexer::types::Tab) -> AgentPadRole {
    use codeterm_types::agent::role_kind::builtins;
    match (tab.role, tab.role_kind_id()) {
        (Some(crate::multiplexer::types::AgentRole::Worker), _) => AgentPadRole::Worker,
        (Some(_), Some(builtins::ORCHESTRATOR)) => AgentPadRole::Orchestrator,
        (Some(_), Some(builtins::GENERAL_AGENT)) => AgentPadRole::General,
        (Some(_), _) => AgentPadRole::Planner,
        (None, _) => AgentPadRole::General,
    }
}

#[derive(Clone, Debug)]
pub struct AgentPad {
    root: PathBuf,
    tab_id: String,
    path: PathBuf,
}

impl AgentPad {
    pub fn create(data_dir: &Path, tab_id: &str, role: AgentPadRole) -> Result<Self, String> {
        validate_tab_id(tab_id)?;
        let root = data_dir.join(PAD_SUBDIR);
        fs::create_dir_all(&root).map_err(|e| format!("create AgentPad root: {e}"))?;
        let path = root.join(tab_id);
        if path.exists() {
            return Err(format!("AgentPad already exists for tab {tab_id}"));
        }
        fs::create_dir(&path).map_err(|e| format!("create AgentPad for tab {tab_id}: {e}"))?;
        let pad = Self {
            root,
            tab_id: tab_id.to_string(),
            path,
        };
        if let Err(error) = pad.seed(role) {
            let _ = fs::remove_dir_all(&pad.path);
            return Err(error);
        }
        Ok(pad)
    }

    pub fn open(data_dir: &Path, tab_id: &str) -> Result<Self, String> {
        validate_tab_id(tab_id)?;
        let root = data_dir.join(PAD_SUBDIR);
        let path = root.join(tab_id);
        if !path.is_dir() {
            return Err(format!("AgentPad is unavailable for tab {tab_id}"));
        }
        verify_exact_child(&root, tab_id, &path)?;
        Ok(Self {
            root,
            tab_id: tab_id.to_string(),
            path,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn env_value(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }

    pub fn list(&self) -> Result<Vec<PathBuf>, String> {
        let canonical_root = self
            .path
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad: {e}"))?;
        let mut files = Vec::new();
        collect_regular_files(&canonical_root, &canonical_root, &mut files)?;
        files.sort();
        Ok(files)
    }

    pub fn read_utf8(&self, relative: &Path) -> Result<String, String> {
        validate_relative_file_path(relative)?;
        let canonical_root = self
            .path
            .canonicalize()
            .map_err(|e| format!("canonicalize AgentPad: {e}"))?;
        let requested = canonical_root.join(relative);
        let canonical_file = requested
            .canonicalize()
            .map_err(|e| format!("resolve AgentPad file {}: {e}", relative.display()))?;
        if !canonical_file.starts_with(&canonical_root) || canonical_file == canonical_root {
            return Err("AgentPad path escapes the current tab pad".into());
        }
        let metadata =
            fs::metadata(&canonical_file).map_err(|e| format!("inspect AgentPad file: {e}"))?;
        if !metadata.is_file() {
            return Err("AgentPad source must be a regular file".into());
        }
        fs::read_to_string(&canonical_file).map_err(|e| format!("read AgentPad UTF-8 file: {e}"))
    }

    pub fn revoke(self) -> Result<(), String> {
        revoke(&self.root, &self.tab_id)
    }

    fn seed(&self, role: AgentPadRole) -> Result<(), String> {
        let mut names = vec!["message.md", "notes.md"];
        if role == AgentPadRole::Worker {
            names.push("report.md");
        }
        if matches!(role, AgentPadRole::Orchestrator | AgentPadRole::Planner) {
            names.push("dispatch.md");
        }
        for name in names {
            fs::write(self.path.join(name), b"")
                .map_err(|e| format!("seed AgentPad {name}: {e}"))?;
        }
        Ok(())
    }
}

pub(crate) fn restore(
    data_dir: &Path,
    tab_id: &str,
    role: AgentPadRole,
) -> Result<AgentPad, String> {
    AgentPad::open(data_dir, tab_id).or_else(|_| AgentPad::create(data_dir, tab_id, role))
}

impl super::ServiceContext {
    pub(crate) fn ensure_agent_pad(&self, tab_id: &str) -> Result<AgentPad, String> {
        let role = self
            .mux
            .read()
            .get_tab(tab_id)
            .map(pad_role_for_tab)
            .unwrap_or(AgentPadRole::General);
        restore(&crate::paths::data_dir(), tab_id, role)
    }

    pub(crate) fn revoke_agent_pad(&self, tab_id: &str) -> Result<(), String> {
        revoke(&crate::paths::data_dir().join(PAD_SUBDIR), tab_id)
    }
}

pub fn revoke(root: &Path, tab_id: &str) -> Result<(), String> {
    validate_tab_id(tab_id)?;
    let path = root.join(tab_id);
    if !path.exists() {
        return Ok(());
    }
    verify_exact_child(root, tab_id, &path)?;
    match fs::remove_dir_all(&path) {
        Ok(()) => Ok(()),
        Err(remove_error) => {
            let tombstone = root.join(format!(
                "{TOMBSTONE_PREFIX}{tab_id}-{}",
                uuid::Uuid::new_v4()
            ));
            fs::rename(&path, &tombstone).map_err(|rename_error| {
                format!("revoke AgentPad: {remove_error}; tombstone rename: {rename_error}")
            })
        }
    }
}

pub fn reconcile(data_dir: &Path, live_tab_ids: &HashSet<String>) -> Result<(), String> {
    let root = data_dir.join(PAD_SUBDIR);
    if !root.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(&root).map_err(|e| format!("read AgentPad root: {e}"))? {
        let entry = entry.map_err(|e| format!("read AgentPad entry: {e}"))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with(TOMBSTONE_PREFIX) || !live_tab_ids.contains(&name) {
            let path = entry.path();
            if path.parent() != Some(root.as_path()) {
                return Err("invalid AgentPad reconciliation target".into());
            }
            if path.is_dir() {
                fs::remove_dir_all(path)
            } else {
                fs::remove_file(path)
            }
            .map_err(|e| format!("remove orphan AgentPad {name}: {e}"))?;
        }
    }
    Ok(())
}

fn validate_tab_id(tab_id: &str) -> Result<(), String> {
    let path = Path::new(tab_id);
    if tab_id.is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err("AgentPad requires a canonical full tab id".into());
    }
    Ok(())
}

fn validate_relative_file_path(path: &Path) -> Result<(), String> {
    if path.as_os_str().is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::Normal(_)))
    {
        return Err("--from-pad must be a non-empty relative file path without traversal".into());
    }
    Ok(())
}

fn verify_exact_child(root: &Path, tab_id: &str, path: &Path) -> Result<(), String> {
    if path.parent() != Some(root) || path.file_name().and_then(|v| v.to_str()) != Some(tab_id) {
        return Err("refusing AgentPad operation outside its exact tab child".into());
    }
    Ok(())
}

fn collect_regular_files(root: &Path, dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("list AgentPad: {e}"))? {
        let entry = entry.map_err(|e| format!("list AgentPad entry: {e}"))?;
        let canonical = entry
            .path()
            .canonicalize()
            .map_err(|e| format!("resolve AgentPad entry: {e}"))?;
        if !canonical.starts_with(root) {
            return Err("AgentPad entry escapes the current tab pad".into());
        }
        let metadata =
            fs::metadata(&canonical).map_err(|e| format!("inspect AgentPad entry: {e}"))?;
        if metadata.is_dir() {
            collect_regular_files(root, &canonical, out)?;
        } else if metadata.is_file() {
            let relative = canonical.strip_prefix(root).unwrap();
            if relative.to_str().is_none() {
                return Err("AgentPad file name is not UTF-8".into());
            }
            out.push(relative.to_path_buf());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
