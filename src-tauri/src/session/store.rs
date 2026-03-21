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
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        /// Claude Code session ID for --resume
        #[serde(skip_serializing_if = "Option::is_none")]
        claude_session_id: Option<String>,
    },
    Split {
        orientation: String,
        ratio: f64,
        children: Vec<SplitNodeSnapshot>,
    },
}

pub struct SessionStore {
    session_dir: PathBuf,
}

impl SessionStore {
    pub fn new() -> Self {
        let dir = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("codeterm")
            .join("sessions");
        let _ = fs::create_dir_all(&dir);
        Self { session_dir: dir }
    }

    pub fn save(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        let path = self.session_dir.join("last.json");
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
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
