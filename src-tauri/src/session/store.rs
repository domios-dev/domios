use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: String,
    pub timestamp: u64,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub active_workspace_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub name: String,
    pub tabs: Vec<TabSnapshot>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub protected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub id: String,
    pub title: String,
    pub split_tree: SplitNodeSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SplitNodeSnapshot {
    Leaf {
        pane_id: String,
        cwd: Option<String>,
        /// Pane title.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title: Option<String>,
        /// Title priority level.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title_level: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        /// Agent session ID for --resume (backward compat: reads "claude_session_id" too).
        #[serde(skip_serializing_if = "Option::is_none", alias = "agent_session_id")]
        claude_session_id: Option<String>,
        /// Orchestration role.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        role: Option<String>,
        /// Parent pane ID in hierarchy.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        parent_pane_id: Option<String>,
        /// Can spawn children.
        #[serde(default)]
        can_spawn: bool,
        /// Hierarchy depth (0=none, 1=orchestrator, 2=lead/worker, 3=worker).
        #[serde(default)]
        depth: u8,
    },
    Split {
        orientation: String,
        ratio: f64,
        children: Vec<SplitNodeSnapshot>,
    },
}

const MAX_BACKUPS: usize = 200;

pub struct SessionStore {
    session_dir: PathBuf,
}

impl SessionStore {
    pub fn new() -> Self {
        let dir = crate::paths::data_dir().join("sessions");
        let _ = fs::create_dir_all(&dir);
        Self { session_dir: dir }
    }

    pub fn save(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        let path = self.session_dir.join("last.json");
        // Rotate: backup current last.json before overwriting
        if path.exists() {
            let _ = self.rotate_backup(&path);
        }
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    fn rotate_backup(&self, last_json: &std::path::Path) -> Result<(), String> {
        let backup_dir = self.session_dir.join("backups");
        let _ = fs::create_dir_all(&backup_dir);

        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup_path = backup_dir.join(format!("{}.json", ts));
        fs::copy(last_json, &backup_path).map_err(|e| e.to_string())?;

        self.evict_old_backups(&backup_dir);
        Ok(())
    }

    fn evict_old_backups(&self, backup_dir: &std::path::Path) {
        let mut entries: Vec<_> = fs::read_dir(backup_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .ends_with(".json")
            })
            .collect();

        if entries.len() <= MAX_BACKUPS {
            return;
        }

        // Sort by name (timestamp) ascending — oldest first
        entries.sort_by_key(|e| e.file_name());
        let to_remove = entries.len() - MAX_BACKUPS;
        for entry in entries.into_iter().take(to_remove) {
            let _ = fs::remove_file(entry.path());
        }
    }

    pub fn load_last(&self) -> Result<Option<SessionSnapshot>, String> {
        let path = self.session_dir.join("last.json");
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let snap = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(Some(snap))
    }

    pub fn save_named(&self, name: &str, snapshot: &SessionSnapshot) -> Result<(), String> {
        let safe_name = name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_");
        let path = self.session_dir.join(format!("{}.json", safe_name));
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn load_named(&self, name: &str) -> Result<Option<SessionSnapshot>, String> {
        let path = self.session_dir.join(format!("{}.json", name));
        if !path.exists() {
            return Ok(None);
        }
        let json = fs::read_to_string(path).map_err(|e| e.to_string())?;
        let snap = serde_json::from_str(&json).map_err(|e| e.to_string())?;
        Ok(Some(snap))
    }

    pub fn list_saved(&self) -> Result<Vec<String>, String> {
        let entries = fs::read_dir(&self.session_dir).map_err(|e| e.to_string())?;
        Ok(entries
            .filter_map(|e| {
                let e = e.ok()?;
                if e.file_type().ok()?.is_dir() {
                    return None; // skip backups/ and other dirs
                }
                let name = e.file_name().to_string_lossy().to_string();
                if name.ends_with(".json") && name != "last.json" {
                    Some(name.trim_end_matches(".json").to_string())
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn delete_saved(&self, name: &str) -> Result<(), String> {
        let path = self.session_dir.join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}
