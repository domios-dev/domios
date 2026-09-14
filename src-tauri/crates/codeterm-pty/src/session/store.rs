use codeterm_types::team::Team;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

fn legacy_snapshot_audience() -> crate::multiplexer::types::TabAudience {
    crate::multiplexer::types::TabAudience::Automation
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnapshot {
    pub version: String,
    pub timestamp: u64,
    pub workspaces: Vec<WorkspaceSnapshot>,
    pub active_workspace_id: Option<String>,
    /// Registered teams. Legacy snapshots lack this and default to empty.
    #[serde(default)]
    pub teams: Vec<Team>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceSnapshot {
    pub id: String,
    pub name: String,
    pub tabs: Vec<TabSnapshot>,
    pub active_tab_id: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_cwd: Option<String>,
    #[serde(default)]
    pub protected: bool,
    /// Sibling ordinal among workspaces. Legacy snapshots default to 0.
    #[serde(default)]
    pub sort_order: i32,
    #[serde(
        default,
        skip_serializing_if = "codeterm_types::plan::task_key::WorkspaceTaskKeys::is_unissued"
    )]
    pub task_keys: codeterm_types::plan::task_key::WorkspaceTaskKeys,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TabSnapshot {
    pub id: String,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub description: Option<String>,
    pub split_tree: SplitNodeSnapshot,
    /// Per-group sibling ordinal. Legacy snapshots default to 0.
    #[serde(default)]
    pub sort_order: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentIdentitySnapshot {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provisioned_cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_binding_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reported_session_binding_nonce: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_state: Option<codeterm_types::pty::AgentLiveState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub boot_stage: Option<codeterm_types::pty::BootStage>,
    #[serde(
        default,
        skip_serializing_if = "crate::multiplexer::types::ReasoningEffortIdentity::is_empty"
    )]
    pub reasoning_effort: crate::multiplexer::types::ReasoningEffortIdentity,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_epochs: Vec<crate::multiplexer::types::ContextEpochArchive>,
    /// Rename-proof assignment identity, persisted so a restart keeps the lane
    /// matchable by task rather than by its mutable title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub task_binding: Option<crate::multiplexer::types::TaskBinding>,
    /// Whether this pane is a tracked roleless agent (parent + audit assigned).
    #[serde(default)]
    pub roleless_tracked: bool,
    /// Lifecycle lineage this snapshot was written under. Recovery matches it
    /// against the journal; absent lineage is unproven, never permissive.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_epoch: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_operation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PersistedAgentSessionIdentity {
    pub provider: String,
    pub role: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role_profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent_tab_id: Option<String>,
    pub can_spawn: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct AgentSessionIdentityCatalog {
    #[serde(default)]
    sessions: HashMap<String, PersistedAgentSessionIdentity>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SplitNodeSnapshot {
    Leaf {
        tab_id: String,
        cwd: Option<String>,
        /// Tab title.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title: Option<String>,
        /// Short tab description shown as tab tooltip/subtitle.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        description: Option<String>,
        /// Title priority level.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        title_level: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        agent_type: Option<String>,
        /// Whether `agent_type` was set via explicit intent (spawn or typed launch
        /// command). Locked tabs ignore output-based detection. Defaults false
        /// so legacy snapshots deserialize cleanly; restore code upgrades them
        /// in-place when an `agent_type` was present.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        agent_type_locked: bool,
        /// Agent session ID for --resume (backward compat: reads "claude_session_id" too).
        #[serde(skip_serializing_if = "Option::is_none", alias = "agent_session_id")]
        claude_session_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_identity: Option<AgentIdentitySnapshot>,
        /// Orchestration role.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        role: Option<String>,
        /// Behavioral role profile, e.g. planner.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        role_profile: Option<String>,
        /// Team membership id. `None` for teamless / legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        team_id: Option<String>,
        /// Parent tab ID in hierarchy.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        parent_tab_id: Option<String>,
        /// Agent that ran `agent spawn` for this tab. Persisted so a restarted
        /// orchestrator keeps its parent/child map and reports still route to
        /// the spawner even with no role parent. `None` for legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        spawned_by_tab_id: Option<String>,
        /// Non-hierarchy report target (cron / mem worker). Persisted alongside
        /// `spawned_by_tab_id` so report routing survives restart. `None` for
        /// legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        report_target_tab_id: Option<String>,
        /// Paired mesh peer the report target lives on (remote worker). Persisted
        /// so a restarted remote worker still relays its reports back across the
        /// mesh. `None` for local tabs / legacy snapshots.
        #[serde(skip_serializing_if = "Option::is_none", default)]
        report_target_mesh_peer: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none", default)]
        ownership: Option<codeterm_types::pty::RemoteOwnership>,
        /// Can spawn children.
        #[serde(default)]
        can_spawn: bool,
        /// Hierarchy depth (0=none, 1=orchestrator, 2=lead/worker, 3=worker).
        #[serde(default)]
        depth: u8,
        /// Whether this tab should restore in Chat UI mode. `None` means
        /// legacy snapshot from before this field existed.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chat_view: Option<bool>,
        /// Whether the agent runs in auto-approve / bypass-permissions mode.
        /// Persisted so a restored agent keeps `--dangerously-skip-permissions`
        /// behaviour instead of silently dropping back to prompting.
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        auto_approve: bool,
        /// Missing legacy audience fails closed as automation.
        #[serde(default = "legacy_snapshot_audience")]
        audience: crate::multiplexer::types::TabAudience,
        /// Model family on the live session. Persisted so resume re-applies it
        /// instead of dropping the tab to the provider default.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_session_model: Option<String>,
        /// Plan/task routing model before a dispatch override.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_planned_model: Option<String>,
        /// Model requested at launch. Advisory; actual identity is agent_session_model.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_requested_model: Option<String>,
        /// Provider catalog result passed to the launch command.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        agent_provider_resolved_model: Option<String>,
        /// Structured fallback provenance, including runtime actual once known.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_fallback: Option<crate::multiplexer::types::ModelFallbackNotice>,
        /// Provider runtime proved the actual model differs from the request.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        model_identity_mismatch: Option<crate::multiplexer::types::ModelIdentityMismatch>,
        /// chatBackend (shell) tab backing — persisted so the tab restores as a
        /// chatBackend (not a PTY) and can lazily re-open its plugin session on
        /// first send. `None` for ordinary PTY tabs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chatbackend_plugin_id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        chatbackend_session_id: Option<String>,
        /// Watcher-mode machine state for chatBackend tabs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        watcher: Option<crate::multiplexer::types::WatcherSpec>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pending_delivery: Option<crate::multiplexer::types::PendingDeliveryRecord>,
    },
    Split {
        orientation: String,
        ratio: f64,
        children: Vec<SplitNodeSnapshot>,
    },
}

const MAX_BACKUPS: usize = 200;
const MAX_REDUCED_QUARANTINE: usize = 20;

/// Whether a snapshot that drops workspaces/tabs vs the persisted state may be
/// written. Only a deliberate removal (user/policy close verb, startup cleanup
/// of known-ephemeral tabs) may reduce; every other save path must refuse, so
/// a wrongly-pruned in-memory state can never destroy the desk on disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveIntent {
    PreserveDesk,
    AuthorizedReduction,
}

/// A refused reducing save. The fuller desk on disk stayed authoritative —
/// this is the guard succeeding, not the save failing.
#[derive(Debug, Clone)]
pub struct ReductionRefusal {
    pub dropped_workspaces: usize,
    pub dropped_tabs: usize,
    pub quarantine: String,
}

impl std::fmt::Display for ReductionRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "refused reducing session save without authorized-reduction intent: would drop \
             {} workspace(s) and {} tab(s); rejected snapshot kept at {}",
            self.dropped_workspaces, self.dropped_tabs, self.quarantine
        )
    }
}

/// How a save ended. `Err` is reserved for genuine failures (I/O,
/// serialization) where the state of the disk is unknown.
#[derive(Debug, Clone)]
pub enum SaveOutcome {
    Saved,
    RefusedReduction(ReductionRefusal),
}

/// One entry of `backups/` — a previously persisted full-desk snapshot.
#[derive(Debug, Clone, Serialize)]
pub struct SessionBackupInfo {
    /// File stem (unix seconds at rotation time); the handle `restore` takes.
    pub name: String,
    pub saved_at_secs: u64,
    pub workspaces: usize,
    pub tabs: usize,
}

pub struct SessionStore {
    pub(crate) session_dir: PathBuf,
}

impl SessionStore {
    pub fn new() -> Self {
        let dir = codeterm_paths::data_dir().join("sessions");
        let _ = fs::create_dir_all(&dir);
        Self { session_dir: dir }
    }

    pub fn for_test_dir(dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&dir);
        Self { session_dir: dir }
    }

    pub fn save(&self, snapshot: &SessionSnapshot) -> Result<SaveOutcome, String> {
        self.save_with_intent(snapshot, SaveIntent::PreserveDesk)
    }

    pub fn save_with_intent(
        &self,
        snapshot: &SessionSnapshot,
        intent: SaveIntent,
    ) -> Result<SaveOutcome, String> {
        if intent == SaveIntent::PreserveDesk {
            if let Some(refusal) = self.detect_reducing_snapshot(snapshot) {
                return Ok(SaveOutcome::RefusedReduction(refusal));
            }
        }
        self.merge_agent_session_identities(snapshot)?;
        let path = self.session_dir.join("last.json");
        let tmp = self.session_dir.join("last.json.tmp");
        if path.exists() {
            let _ = self.rotate_backup(&path);
        }
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        // Atomic write: tmp file then rename. fs::write directly into last.json is NOT crash-safe — a kill mid-write (Windows reboot, OOM, BSOD)
        // truncates the live file and the next boot loses every workspace.
        fs::write(&tmp, json.as_bytes()).map_err(|e| format!("write {}: {}", tmp.display(), e))?;
        fs::rename(&tmp, &path).map_err(|e| format!("rename → {}: {}", path.display(), e))?;
        Ok(SaveOutcome::Saved)
    }

    /// Some(refusal) when `snapshot` drops workspaces/tabs vs the persisted
    /// `last.json`; the rejected snapshot is quarantined next to it and the
    /// refusal logged at error before returning.
    fn detect_reducing_snapshot(&self, snapshot: &SessionSnapshot) -> Option<ReductionRefusal> {
        let path = self.session_dir.join("last.json");
        let Ok(json) = fs::read_to_string(&path) else {
            return None;
        };
        // A corrupt persisted snapshot can't anchor a comparison; the new save
        // is strictly better than an unreadable file.
        let Ok(persisted) = parse_session_snapshot(&json, &path) else {
            return None;
        };

        let new_workspace_ids: std::collections::HashSet<&str> = snapshot
            .workspaces
            .iter()
            .map(|ws| ws.id.as_str())
            .collect();
        // Tab ids compare globally so a tab moved across workspaces is not a drop.
        let new_tab_ids: std::collections::HashSet<&str> = snapshot
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.iter().map(|tab| tab.id.as_str()))
            .collect();
        let dropped_workspaces = persisted
            .workspaces
            .iter()
            .filter(|ws| !new_workspace_ids.contains(ws.id.as_str()))
            .count();
        let dropped_tabs = persisted
            .workspaces
            .iter()
            .flat_map(|ws| ws.tabs.iter())
            .filter(|tab| !new_tab_ids.contains(tab.id.as_str()))
            .count();
        if dropped_workspaces == 0 && dropped_tabs == 0 {
            return None;
        }

        let refusal = ReductionRefusal {
            dropped_workspaces,
            dropped_tabs,
            quarantine: self.quarantine_reduced_snapshot(snapshot),
        };
        log::error!(
            "[session] REFUSED: {refusal} (persisted desk {} stays authoritative)",
            path.display()
        );
        Some(refusal)
    }

    fn quarantine_reduced_snapshot(&self, snapshot: &SessionSnapshot) -> String {
        let epoch_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let path = self
            .session_dir
            .join(format!("last.json.reduced-{epoch_ms}"));
        match serde_json::to_string_pretty(snapshot)
            .map_err(|e| e.to_string())
            .and_then(|json| fs::write(&path, json).map_err(|e| e.to_string()))
        {
            Ok(()) => {
                self.evict_old_reduced_quarantine();
                path.display().to_string()
            }
            Err(error) => format!("<quarantine write failed: {error}>"),
        }
    }

    fn evict_old_reduced_quarantine(&self) {
        let mut entries: Vec<_> = fs::read_dir(&self.session_dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("last.json.reduced-")
            })
            .collect();
        if entries.len() <= MAX_REDUCED_QUARANTINE {
            return;
        }
        entries.sort_by_key(|e| e.file_name());
        let to_remove = entries.len() - MAX_REDUCED_QUARANTINE;
        for entry in entries.into_iter().take(to_remove) {
            let _ = fs::remove_file(entry.path());
        }
    }

    pub fn load_agent_session_identity(
        &self,
        provider: &str,
        session_id: &str,
    ) -> Option<PersistedAgentSessionIdentity> {
        self.load_agent_session_identity_catalog()
            .sessions
            .get(&agent_session_identity_key(provider, session_id))
            .cloned()
    }

    fn merge_agent_session_identities(&self, snapshot: &SessionSnapshot) -> Result<(), String> {
        let mut catalog = self.load_agent_session_identity_catalog();
        for workspace in &snapshot.workspaces {
            for tab in &workspace.tabs {
                collect_agent_session_identities(&tab.split_tree, &mut catalog.sessions);
            }
        }
        let path = self.session_dir.join("agent-session-identities.json");
        let tmp = self.session_dir.join("agent-session-identities.json.tmp");
        let backup = self.session_dir.join("agent-session-identities.json.bak");
        let json = serde_json::to_string_pretty(&catalog).map_err(|error| error.to_string())?;
        fs::write(&tmp, json.as_bytes())
            .map_err(|error| format!("write {}: {error}", tmp.display()))?;
        if backup.exists() {
            fs::remove_file(&backup)
                .map_err(|error| format!("remove {}: {error}", backup.display()))?;
        }
        if path.exists() {
            fs::rename(&path, &backup)
                .map_err(|error| format!("backup {}: {error}", path.display()))?;
        }
        fs::rename(&tmp, &path).map_err(|error| format!("rename → {}: {error}", path.display()))?;
        if backup.exists() {
            let _ = fs::remove_file(backup);
        }
        Ok(())
    }

    fn load_agent_session_identity_catalog(&self) -> AgentSessionIdentityCatalog {
        let path = self.session_dir.join("agent-session-identities.json");
        let backup = self.session_dir.join("agent-session-identities.json.bak");
        for candidate in [&path, &backup] {
            let Ok(json) = fs::read_to_string(candidate) else {
                continue;
            };
            match serde_json::from_str(&json) {
                Ok(catalog) => return catalog,
                Err(error) => log::error!(
                    "[session] ignored corrupt agent session identity catalog {}: {}",
                    candidate.display(),
                    error
                ),
            }
        }
        AgentSessionIdentityCatalog::default()
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

        entries.sort_by_key(|e| e.file_name());
        let to_remove = entries.len() - MAX_BACKUPS;
        for entry in entries.into_iter().take(to_remove) {
            let _ = fs::remove_file(entry.path());
        }
    }

    /// Rotated `backups/` snapshots, newest first. Unparseable files are skipped.
    pub fn list_backups(&self) -> Result<Vec<SessionBackupInfo>, String> {
        let backup_dir = self.session_dir.join("backups");
        if !backup_dir.exists() {
            return Ok(Vec::new());
        }
        let mut infos: Vec<SessionBackupInfo> = fs::read_dir(&backup_dir)
            .map_err(|e| format!("read backups: {}", e))?
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let file_name = entry.file_name().to_string_lossy().to_string();
                let name = file_name.strip_suffix(".json")?.to_string();
                let saved_at_secs: u64 = name.parse().ok()?;
                let content = fs::read_to_string(entry.path()).ok()?;
                let snap = parse_session_snapshot(&content, &entry.path()).ok()?;
                Some(SessionBackupInfo {
                    name,
                    saved_at_secs,
                    workspaces: snap.workspaces.len(),
                    tabs: snap.workspaces.iter().map(|ws| ws.tabs.len()).sum(),
                })
            })
            .collect();
        infos.sort_by(|a, b| b.saved_at_secs.cmp(&a.saved_at_secs));
        Ok(infos)
    }

    /// Load one `backups/` snapshot by its `list_backups` name.
    pub fn load_backup(&self, name: &str) -> Result<SessionSnapshot, String> {
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_digit()) {
            return Err(format!("invalid backup name: {name}"));
        }
        let path = self
            .session_dir
            .join("backups")
            .join(format!("{name}.json"));
        let json = fs::read_to_string(&path)
            .map_err(|e| format!("read backup {}: {}", path.display(), e))?;
        parse_session_snapshot(&json, &path)
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
        match parse_session_snapshot(&json, &path) {
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
            let Ok(snap) = parse_session_snapshot(&content, &path) else {
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
        if safe_name == "agent-session-identities" {
            return Err("reserved session identity catalog".into());
        }
        let path = self.session_dir.join(format!("{}.json", safe_name));
        let json = serde_json::to_string_pretty(snapshot).map_err(|e| e.to_string())?;
        fs::write(path, json).map_err(|e| e.to_string())
    }

    pub fn load_named(&self, name: &str) -> Result<Option<SessionSnapshot>, String> {
        if name == "agent-session-identities" {
            return Err("reserved session identity catalog".into());
        }
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
                if name.ends_with(".json")
                    && name != "last.json"
                    && name != "agent-session-identities.json"
                {
                    Some(name.trim_end_matches(".json").to_string())
                } else {
                    None
                }
            })
            .collect())
    }

    pub fn delete_saved(&self, name: &str) -> Result<(), String> {
        if name == "agent-session-identities" {
            return Err("reserved session identity catalog".into());
        }
        let path = self.session_dir.join(format!("{}.json", name));
        if path.exists() {
            fs::remove_file(path).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
}

fn agent_session_identity_key(provider: &str, session_id: &str) -> String {
    format!("{}:{provider}{session_id}", provider.len())
}

fn collect_agent_session_identities(
    node: &SplitNodeSnapshot,
    sessions: &mut HashMap<String, PersistedAgentSessionIdentity>,
) {
    match node {
        SplitNodeSnapshot::Split { children, .. } => {
            for child in children {
                collect_agent_session_identities(child, sessions);
            }
        }
        SplitNodeSnapshot::Leaf {
            agent_type: Some(provider),
            claude_session_id: Some(session_id),
            agent_identity: Some(binding),
            role,
            role_profile,
            parent_tab_id,
            can_spawn,
            ..
        } if binding.session_binding_nonce.is_some()
            && binding.session_binding_nonce == binding.reported_session_binding_nonce =>
        {
            let key = agent_session_identity_key(provider, session_id);
            if let Some(role) = role {
                sessions.insert(
                    key,
                    PersistedAgentSessionIdentity {
                        provider: provider.clone(),
                        role: role.clone(),
                        role_profile: role_profile.clone(),
                        parent_tab_id: parent_tab_id.clone(),
                        can_spawn: *can_spawn,
                    },
                );
            } else {
                sessions.remove(&key);
            }
        }
        SplitNodeSnapshot::Leaf { .. } => {}
    }
}

fn parse_session_snapshot(json: &str, path: &std::path::Path) -> Result<SessionSnapshot, String> {
    let mut root: serde_json::Value =
        serde_json::from_str(json).map_err(|error| error.to_string())?;
    let Some(root_object) = root.as_object_mut() else {
        return Err("snapshot root is not an object".into());
    };
    let workspaces = root_object
        .remove("workspaces")
        .ok_or_else(|| "snapshot has no workspaces field".to_string())?;
    let workspaces = workspaces
        .as_array()
        .ok_or_else(|| "snapshot workspaces field is not an array".to_string())?;
    let mut loaded = Vec::new();
    for (index, workspace) in workspaces.iter().enumerate() {
        if let Some(workspace) = parse_workspace_record(path, index, workspace.clone()) {
            loaded.push(workspace);
        }
    }
    root_object.insert(
        "workspaces".into(),
        serde_json::to_value(loaded).map_err(|error| error.to_string())?,
    );
    serde_json::from_value(root).map_err(|error| error.to_string())
}

fn parse_workspace_record(
    path: &std::path::Path,
    index: usize,
    mut workspace: serde_json::Value,
) -> Option<WorkspaceSnapshot> {
    let workspace_id = workspace
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    let Some(workspace_object) = workspace.as_object_mut() else {
        log::error!(
            "[session] quarantined workspace record {} at index {} in {}: expected object",
            workspace_id,
            index,
            path.display()
        );
        return None;
    };
    let tabs = match workspace_object.remove("tabs") {
        Some(serde_json::Value::Array(tabs)) => tabs,
        _ => {
            log::error!(
                "[session] quarantined workspace record {} at index {} in {}: missing tabs array",
                workspace_id,
                index,
                path.display()
            );
            return None;
        }
    };
    if let Err(error) = codeterm_types::startup_records::normalize_pane_id_fields(&mut workspace) {
        log::error!(
            "[session] quarantined workspace record {} at index {} in {}: {}",
            workspace_id,
            index,
            path.display(),
            error
        );
        return None;
    }
    let loaded_tabs: Vec<TabSnapshot> = tabs
        .into_iter()
        .enumerate()
        .filter_map(|(tab_index, tab)| parse_tab_record(path, &workspace_id, tab_index, tab))
        .collect();
    let loaded_tabs = match serde_json::to_value(loaded_tabs) {
        Ok(loaded_tabs) => loaded_tabs,
        Err(error) => {
            log::error!(
                "[session] quarantined workspace record {} at index {} in {}: {}",
                workspace_id,
                index,
                path.display(),
                error
            );
            return None;
        }
    };
    let Some(workspace_object) = workspace.as_object_mut() else {
        log::error!(
            "[session] quarantined workspace record {} at index {} in {}: object shape changed during normalization",
            workspace_id,
            index,
            path.display()
        );
        return None;
    };
    workspace_object.insert("tabs".into(), loaded_tabs);
    match serde_json::from_value(workspace) {
        Ok(workspace) => Some(workspace),
        Err(error) => {
            log::error!(
                "[session] quarantined workspace record {} at index {} in {}: {}",
                workspace_id,
                index,
                path.display(),
                error
            );
            None
        }
    }
}

fn parse_tab_record(
    path: &std::path::Path,
    workspace_id: &str,
    index: usize,
    mut tab: serde_json::Value,
) -> Option<TabSnapshot> {
    let tab_id = tab
        .get("id")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    let normalized = match codeterm_types::startup_records::normalize_pane_id_fields(&mut tab) {
        Ok(normalized) => normalized,
        Err(error) => {
            log::error!(
                "[session] quarantined tab record {}/{} at index {} in {}: {}",
                workspace_id,
                tab_id,
                index,
                path.display(),
                error
            );
            return None;
        }
    };
    match serde_json::from_value(tab) {
        Ok(tab) => {
            if normalized > 0 {
                log::warn!(
                    "[session] normalized {} pane-era fields in tab record {}/{} from {}; source left unchanged",
                    normalized,
                    workspace_id,
                    tab_id,
                    path.display()
                );
            }
            Some(tab)
        }
        Err(error) => {
            log::error!(
                "[session] quarantined tab record {}/{} at index {} in {}: {}; source left unchanged",
                workspace_id,
                tab_id,
                index,
                path.display(),
                error
            );
            None
        }
    }
}

#[cfg(test)]
mod tests;
