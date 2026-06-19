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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
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
        /// Short pane description shown as tab tooltip/subtitle.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        description: Option<String>,
        /// Title priority level.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title_level: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        /// Whether `agent_type` was set via explicit intent (spawn or typed launch
        /// command). Locked panes ignore output-based detection. Defaults false
        /// so legacy snapshots deserialize cleanly; restore code upgrades them
        /// in-place when an `agent_type` was present.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        agent_type_locked: bool,
        /// Agent session ID for --resume (backward compat: reads "claude_session_id" too).
        #[serde(skip_serializing_if = "Option::is_none", alias = "agent_session_id")]
        claude_session_id: Option<String>,
        /// Orchestration role.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        role: Option<String>,
        /// Parent pane ID in hierarchy.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        parent_pane_id: Option<String>,
        /// Agent that ran `agent spawn` for this pane. Persisted so a restarted
        /// orchestrator keeps its parent/child map and reports still route to
        /// the spawner even with no role parent. `None` for legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        spawned_by_pane_id: Option<String>,
        /// Non-hierarchy report target (cron / mem worker). Persisted alongside
        /// `spawned_by_pane_id` so report routing survives restart. `None` for
        /// legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        report_target_pane_id: Option<String>,
        /// Can spawn children.
        #[serde(default)]
        can_spawn: bool,
        /// Hierarchy depth (0=none, 1=orchestrator, 2=lead/worker, 3=worker).
        #[serde(default)]
        depth: u8,
        /// Whether this pane should restore in Chat UI mode. `None` means
        /// legacy snapshot from before this field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat_view: Option<bool>,
        /// Whether the agent runs in auto-approve / bypass-permissions mode.
        /// Persisted so a restored agent keeps `--dangerously-skip-permissions`
        /// behaviour instead of silently dropping back to prompting.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto_approve: bool,
        /// Model family on the live session. Persisted so resume re-applies it
        /// instead of dropping the pane to the provider default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_session_model: Option<String>,
        /// chatBackend (shell) pane backing — persisted so the pane restores as a
        /// chatBackend (not a PTY) and can lazily re-open its plugin session on
        /// first send. `None` for ordinary PTY panes.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chatbackend_plugin_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chatbackend_session_id: Option<String>,
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
        let tmp = self.session_dir.join("last.json.tmp");
        if path.exists() {
            let _ = self.rotate_backup(&path);
        }
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        // Atomic write: tmp file then rename. fs::write directly into last.json is NOT crash-safe — a kill mid-write (Windows reboot, OOM, BSOD)
        // truncates the live file and the next boot loses every workspace.
        fs::write(&tmp, json.as_bytes()).map_err(|e| format!("write {}: {}", tmp.display(), e))?;
        fs::rename(&tmp, &path).map_err(|e| format!("rename → {}: {}", path.display(), e))
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
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
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
            return self.load_latest_valid_backup();
        }
        let json = match fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                log::error!(
                    "[session] last.json read failed ({}); falling back to backups/",
                    e
                );
                return self.load_latest_valid_backup();
            }
        };
        match serde_json::from_str::<SessionSnapshot>(&json) {
            Ok(snap) => Ok(Some(snap)),
            Err(parse_err) => {
                log::error!(
                    "[session] last.json corrupt ({} bytes, parse error: {}); falling back to backups/",
                    json.len(),
                    parse_err
                );
                self.load_latest_valid_backup()
            }
        }
    }

    fn load_latest_valid_backup(&self) -> Result<Option<SessionSnapshot>, String> {
        let backup_dir = self.session_dir.join("backups");
        if !backup_dir.exists() {
            log::error!("[session] no backups/ dir, cannot recover; starting empty");
            return Ok(None);
        }
        let mut entries: Vec<_> = fs::read_dir(&backup_dir)
            .map_err(|e| format!("read backups: {}", e))?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with(".json"))
            .collect();
        entries.sort_by_key(|e| e.file_name());
        for entry in entries.into_iter().rev() {
            let path = entry.path();
            let Ok(content) = fs::read_to_string(&path) else {
                continue;
            };
            let Ok(snap) = serde_json::from_str::<SessionSnapshot>(&content) else {
                continue;
            };
            log::warn!(
                "[session] recovered from backup {} ({} workspaces)",
                path.display(),
                snap.workspaces.len()
            );
            return Ok(Some(snap));
        }
        log::error!("[session] every file in backups/ failed to parse; starting empty");
        Ok(None)
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn fresh_store() -> (SessionStore, TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore {
            session_dir: dir.path().to_path_buf(),
        };
        (store, dir)
    }

    fn snap(workspaces: usize) -> SessionSnapshot {
        SessionSnapshot {
            version: "test".into(),
            timestamp: 0,
            active_workspace_id: None,
            workspaces: (0..workspaces)
                .map(|i| WorkspaceSnapshot {
                    id: format!("ws_{}", i),
                    name: format!("ws{}", i),
                    tabs: vec![],
                    active_tab_id: None,
                    cwd: None,
                    protected: false,
                })
                .collect(),
        }
    }

    #[test]
    fn save_is_atomic_no_leftover_tmp() {
        let (store, _dir) = fresh_store();
        store.save(&snap(2)).unwrap();
        assert!(store.session_dir.join("last.json").exists());
        assert!(!store.session_dir.join("last.json.tmp").exists());
    }

    #[test]
    fn load_corrupt_last_falls_back_to_backup() {
        let (store, _dir) = fresh_store();
        store.save(&snap(3)).unwrap();
        store.save(&snap(4)).unwrap();
        store.save(&snap(5)).unwrap();

        fs::write(store.session_dir.join("last.json"), b"{not valid json").unwrap();

        let recovered = store.load_last().unwrap().expect("must recover");
        assert!(
            !recovered.workspaces.is_empty(),
            "fallback to backups must return non-empty snapshot"
        );
    }

    #[test]
    fn load_missing_last_falls_back_to_backup() {
        let (store, _dir) = fresh_store();
        store.save(&snap(2)).unwrap();
        store.save(&snap(3)).unwrap();
        fs::remove_file(store.session_dir.join("last.json")).unwrap();

        let recovered = store
            .load_last()
            .unwrap()
            .expect("must recover from backup");
        assert!(!recovered.workspaces.is_empty());
    }

    #[test]
    fn load_no_last_no_backups_returns_none() {
        let (store, _dir) = fresh_store();
        let res = store.load_last().unwrap();
        assert!(res.is_none());
    }

    #[test]
    fn load_corrupt_last_and_corrupt_backups_returns_none() {
        let (store, _dir) = fresh_store();
        store.save(&snap(1)).unwrap();
        store.save(&snap(1)).unwrap();
        fs::write(store.session_dir.join("last.json"), b"oops").unwrap();
        let backup_dir = store.session_dir.join("backups");
        for entry in fs::read_dir(&backup_dir).unwrap().flatten() {
            fs::write(entry.path(), b"corrupt").unwrap();
        }
        let res = store.load_last().unwrap();
        assert!(res.is_none(), "all corrupt → None, never panics");
    }

    #[test]
    fn round_trip_save_load() {
        let (store, _dir) = fresh_store();
        let s = snap(3);
        store.save(&s).unwrap();
        let loaded = store.load_last().unwrap().unwrap();
        assert_eq!(loaded.workspaces.len(), 3);
    }
}
