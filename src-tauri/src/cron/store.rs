use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

// ── Types ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum CronRunStatus {
    Running,
    Success,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronRunInfo {
    pub run_id: String,
    pub started_at: u64,
    pub finished_at: Option<u64>,
    pub status: CronRunStatus,
    pub pane_id: Option<String>,
    pub tab_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJob {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub workspace_id: String,
    pub schedule: String,
    pub enabled: bool,
    pub agent: String,
    pub task: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// Pane ID of whoever created this cron — spawned agents report back here.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub creator_pane_id: Option<String>,
    /// If true, the job fires once at `next_run` and is auto-deleted on completion.
    #[serde(default, skip_serializing_if = "is_false")]
    pub oneshot: bool,
    #[serde(default = "default_max_history")]
    pub max_history: usize,
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_run: Option<CronRunInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next_run: Option<u64>,
}

fn default_max_history() -> usize {
    100
}

fn is_false(v: &bool) -> bool {
    !v
}

// ── Persistence envelope ───────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CronFile {
    version: String,
    jobs: Vec<CronJob>,
    history: HashMap<String, Vec<CronRunInfo>>,
}

impl Default for CronFile {
    fn default() -> Self {
        Self {
            version: "1".to_string(),
            jobs: Vec::new(),
            history: HashMap::new(),
        }
    }
}

// ── Cron expression helpers ────────────────────────────────────────

/// Convert 5-field cron ("*/30 * * * *") to 6-field ("0 */30 * * * *")
/// that the `cron` crate expects (sec min hour dom mon dow).
pub fn to_6field(expr: &str) -> String {
    let fields: Vec<&str> = expr.trim().split_whitespace().collect();
    if fields.len() == 5 {
        format!("0 {}", expr.trim())
    } else {
        expr.trim().to_string()
    }
}

/// Parse a cron expression and compute the next fire time (unix ms).
pub fn compute_next_run(schedule: &str) -> Result<Option<u64>, String> {
    use std::str::FromStr;
    let expr = to_6field(schedule);
    let sched = cron::Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    Ok(sched.upcoming(chrono::Utc).next().map(|dt| dt.timestamp_millis() as u64))
}

/// Validate a cron expression without computing next run.
pub fn validate_schedule(schedule: &str) -> Result<(), String> {
    use std::str::FromStr;
    let expr = to_6field(schedule);
    cron::Schedule::from_str(&expr).map_err(|e| format!("Invalid cron: {}", e))?;
    Ok(())
}

/// Parse a oneshot schedule string into an absolute Unix timestamp (ms).
///
/// Accepts:
/// - Relative: "in 5m", "in 1h30m", "in 2h", "in 90s", "in 1d"
/// - ISO 8601: "2025-04-05T14:30:00Z", "2025-04-05T14:30:00+03:00"
/// - Unix ms: "1712345678000" (13+ digits)
pub fn parse_oneshot_schedule(schedule: &str) -> Result<u64, String> {
    let s = schedule.trim();

    // Relative: "in 5m", "in 1h30m", "in 2d12h"
    if let Some(rest) = s.strip_prefix("in ") {
        let duration_ms = parse_relative_duration(rest.trim())?;
        if duration_ms == 0 {
            return Err("Duration must be > 0".into());
        }
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        return Ok(now + duration_ms);
    }

    // Unix timestamp (13+ digit number = ms)
    if s.len() >= 13 && s.chars().all(|c| c.is_ascii_digit()) {
        let ts: u64 = s.parse().map_err(|_| "Invalid timestamp".to_string())?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if ts <= now {
            return Err("Oneshot time must be in the future".into());
        }
        return Ok(ts);
    }

    // ISO 8601
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        let ts = dt.timestamp_millis() as u64;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        if ts <= now {
            return Err("Oneshot time must be in the future".into());
        }
        return Ok(ts);
    }

    Err(format!(
        "Invalid oneshot schedule '{}'. Use: 'in 5m', 'in 1h30m', ISO8601, or unix ms",
        s
    ))
}

/// Parse a relative duration string like "5m", "1h30m", "2d", "90s" into milliseconds.
fn parse_relative_duration(s: &str) -> Result<u64, String> {
    let mut total_ms: u64 = 0;
    let mut num_buf = String::new();

    for c in s.chars() {
        if c.is_ascii_digit() {
            num_buf.push(c);
        } else {
            let n: u64 = num_buf.parse().map_err(|_| format!("Invalid number in duration: '{}'", s))?;
            num_buf.clear();
            match c {
                's' => total_ms += n * 1_000,
                'm' => total_ms += n * 60_000,
                'h' => total_ms += n * 3_600_000,
                'd' => total_ms += n * 86_400_000,
                _ => return Err(format!("Unknown duration unit '{}' in '{}'", c, s)),
            }
        }
    }

    if !num_buf.is_empty() {
        // Bare number without unit — treat as minutes (most intuitive default)
        let n: u64 = num_buf.parse().map_err(|_| format!("Invalid number in duration: '{}'", s))?;
        total_ms += n * 60_000;
    }

    if total_ms == 0 {
        return Err(format!("Could not parse duration from '{}'", s));
    }

    Ok(total_ms)
}

// ── Store ──────────────────────────────────────────────────────────

pub struct CronStore {
    file: CronFile,
    file_path: PathBuf,
    /// Runtime mapping: pane_id → (job_id, run_id). Not persisted.
    pane_owners: HashMap<String, (String, String)>,
}

impl CronStore {
    pub fn new() -> Self {
        let dir = crate::paths::data_dir();
        let file_path = dir.join("crons.json");
        let file = if file_path.exists() {
            match fs::read_to_string(&file_path) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => CronFile::default(),
            }
        } else {
            CronFile::default()
        };
        Self { file, file_path, pane_owners: HashMap::new() }
    }

    #[cfg(test)]
    pub fn new_with_path(path: PathBuf) -> Self {
        let file = if path.exists() {
            match fs::read_to_string(&path) {
                Ok(json) => serde_json::from_str(&json).unwrap_or_default(),
                Err(_) => CronFile::default(),
            }
        } else {
            CronFile::default()
        };
        Self { file, file_path: path, pane_owners: HashMap::new() }
    }

    // ── Pane ownership tracking ─────────────────────────────────

    pub fn register_pane(&mut self, pane_id: &str, job_id: &str, run_id: &str) {
        self.pane_owners.insert(pane_id.to_string(), (job_id.to_string(), run_id.to_string()));
    }

    pub fn get_pane_owner(&self, pane_id: &str) -> Option<&(String, String)> {
        self.pane_owners.get(pane_id)
    }

    pub fn unregister_pane(&mut self, pane_id: &str) {
        self.pane_owners.remove(pane_id);
    }

    /// Get all pane_ids owned by a specific job.
    pub fn pane_owners_for_job(&self, job_id: &str) -> Vec<String> {
        self.pane_owners
            .iter()
            .filter(|(_, (jid, _))| jid == job_id)
            .map(|(pid, _)| pid.clone())
            .collect()
    }

    /// Find (job_id, pane_id) by run_id — reverse lookup through pane_owners.
    pub fn get_pane_owner_by_run(&self, run_id: &str) -> Option<(&str, &str)> {
        for (pane_id, (job_id, rid)) in &self.pane_owners {
            if rid == run_id {
                return Some((job_id.as_str(), pane_id.as_str()));
            }
        }
        None
    }

    /// Get the cron logs directory, creating if needed.
    pub fn logs_dir(&self) -> PathBuf {
        let dir = self.file_path.parent()
            .unwrap_or_else(|| std::path::Path::new("."))
            .join("crons");
        let _ = fs::create_dir_all(&dir);
        dir
    }

    /// Save a cron run log to disk.
    pub fn save_log(&self, job_id: &str, run_id: &str, text: &str) -> Result<PathBuf, String> {
        let job_dir = self.logs_dir().join(job_id);
        let _ = fs::create_dir_all(&job_dir);
        let path = job_dir.join(format!("{}.log", run_id));
        fs::write(&path, text).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// Read a cron run log from disk.
    pub fn read_log(&self, job_id: &str, run_id: &str) -> Option<String> {
        let path = self.logs_dir().join(job_id).join(format!("{}.log", run_id));
        fs::read_to_string(path).ok()
    }

    pub fn save(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.file).map_err(|e| e.to_string())?;
        fs::write(&self.file_path, json).map_err(|e| e.to_string())
    }

    fn now_ms() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64
    }

    // ── CRUD ────────────────────────────────────────────────────

    pub fn create(&mut self, params: CronCreateParams) -> Result<CronJob, String> {
        let oneshot = params.oneshot.unwrap_or(false);

        let next_run = if !params.enabled.unwrap_or(true) {
            if oneshot {
                // Validate oneshot schedule even when disabled
                parse_oneshot_schedule(&params.schedule)?;
            } else {
                validate_schedule(&params.schedule)?;
            }
            None
        } else if oneshot {
            Some(parse_oneshot_schedule(&params.schedule)?)
        } else {
            validate_schedule(&params.schedule)?;
            compute_next_run(&params.schedule)?
        };

        let job = CronJob {
            id: uuid::Uuid::new_v4().to_string(),
            name: params.name,
            description: params.description,
            workspace_id: params.workspace_id,
            schedule: params.schedule,
            enabled: params.enabled.unwrap_or(true),
            agent: params.agent.unwrap_or_else(crate::config::default_agent_provider),
            task: params.task,
            cwd: params.cwd,
            role: params.role,
            creator_pane_id: params.creator_pane_id,
            oneshot,
            max_history: params.max_history.unwrap_or(default_max_history()),
            created_at: Self::now_ms(),
            last_run: None,
            next_run,
        };

        self.file.jobs.push(job.clone());
        self.save()?;
        Ok(job)
    }

    pub fn get(&self, id: &str) -> Option<&CronJob> {
        self.file.jobs.iter().find(|j| j.id == id)
    }

    pub fn list(&self) -> &[CronJob] {
        &self.file.jobs
    }

    pub fn list_for_workspace(&self, workspace_id: &str) -> Vec<&CronJob> {
        self.file
            .jobs
            .iter()
            .filter(|j| j.workspace_id == workspace_id)
            .collect()
    }

    pub fn list_enabled(&self) -> Vec<&CronJob> {
        self.file.jobs.iter().filter(|j| j.enabled).collect()
    }

    pub fn get_mut(&mut self, id: &str) -> Option<&mut CronJob> {
        self.file.jobs.iter_mut().find(|j| j.id == id)
    }

    pub fn get_history_mut(&mut self, job_id: &str) -> Option<&mut Vec<CronRunInfo>> {
        self.file.history.get_mut(job_id)
    }

    pub fn update(&mut self, id: &str, patch: CronUpdateParams) -> Result<CronJob, String> {
        let job = self
            .file
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| format!("Cron job not found: {}", id))?;

        if let Some(name) = patch.name {
            job.name = name;
        }
        if let Some(desc) = patch.description {
            job.description = Some(desc);
        }
        if let Some(schedule) = patch.schedule {
            if job.oneshot {
                parse_oneshot_schedule(&schedule)?;
            } else {
                validate_schedule(&schedule)?;
            }
            job.schedule = schedule;
        }
        if let Some(task) = patch.task {
            job.task = task;
        }
        if let Some(cwd) = patch.cwd {
            job.cwd = Some(cwd);
        }
        if let Some(role) = patch.role {
            job.role = Some(role);
        }
        if let Some(max_history) = patch.max_history {
            job.max_history = max_history;
        }
        if let Some(enabled) = patch.enabled {
            job.enabled = enabled;
        }

        // Recompute next_run
        job.next_run = if !job.enabled {
            None
        } else if job.oneshot {
            Some(parse_oneshot_schedule(&job.schedule)?)
        } else {
            compute_next_run(&job.schedule)?
        };

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    pub fn delete(&mut self, id: &str) -> Result<(), String> {
        let before = self.file.jobs.len();
        self.file.jobs.retain(|j| j.id != id);
        if self.file.jobs.len() == before {
            return Err(format!("Cron job not found: {}", id));
        }
        self.file.history.remove(id);
        self.save()
    }

    pub fn set_enabled(&mut self, id: &str, enabled: bool) -> Result<CronJob, String> {
        let job = self
            .file
            .jobs
            .iter_mut()
            .find(|j| j.id == id)
            .ok_or_else(|| format!("Cron job not found: {}", id))?;

        job.enabled = enabled;
        job.next_run = if !enabled {
            None
        } else if job.oneshot {
            Some(parse_oneshot_schedule(&job.schedule)?)
        } else {
            compute_next_run(&job.schedule)?
        };

        let updated = job.clone();
        self.save()?;
        Ok(updated)
    }

    // ── Run history ─────────────────────────────────────────────

    pub fn record_run(&mut self, job_id: &str, run: CronRunInfo) -> Result<(), String> {
        // Update last_run on job
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            job.last_run = Some(run.clone());
            // Recompute next_run (oneshot: no next run after firing)
            if job.oneshot {
                job.next_run = None;
                job.enabled = false;
            } else if job.enabled {
                job.next_run = compute_next_run(&job.schedule)?;
            }

            let max = job.max_history;
            let history = self.file.history.entry(job_id.to_string()).or_default();
            history.push(run);
            // Evict old entries
            while history.len() > max {
                history.remove(0);
            }
        }
        self.save()
    }

    pub fn update_run_status(
        &mut self,
        job_id: &str,
        run_id: &str,
        status: CronRunStatus,
    ) -> Result<(), String> {
        let now = Self::now_ms();
        if let Some(history) = self.file.history.get_mut(job_id) {
            if let Some(run) = history.iter_mut().find(|r| r.run_id == run_id) {
                run.status = status.clone();
                run.finished_at = Some(now);
            }
        }
        // Also update last_run on the job
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.id == job_id) {
            if let Some(ref mut lr) = job.last_run {
                if lr.run_id == run_id {
                    lr.status = status;
                    lr.finished_at = Some(now);
                }
            }
        }
        self.save()
    }

    pub fn get_history(&self, job_id: &str) -> Vec<&CronRunInfo> {
        self.file
            .history
            .get(job_id)
            .map(|h| h.iter().collect())
            .unwrap_or_default()
    }
}

// ── Params ─────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
pub struct CronCreateParams {
    pub name: String,
    pub workspace_id: String,
    pub schedule: String,
    pub task: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub agent: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub creator_pane_id: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_history: Option<usize>,
    /// If true, fires once at the scheduled time and auto-deletes on completion.
    #[serde(default)]
    pub oneshot: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CronUpdateParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub schedule: Option<String>,
    #[serde(default)]
    pub task: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub role: Option<String>,
    #[serde(default)]
    pub enabled: Option<bool>,
    #[serde(default)]
    pub max_history: Option<usize>,
}

// ── Tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> CronStore {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        // Leak the tempdir so it doesn't get deleted while store is alive
        let path_clone = path.clone();
        std::mem::forget(dir);
        CronStore::new_with_path(path_clone)
    }

    fn make_params(name: &str, ws: &str, schedule: &str) -> CronCreateParams {
        CronCreateParams {
            name: name.to_string(),
            workspace_id: ws.to_string(),
            schedule: schedule.to_string(),
            task: "test task".to_string(),
            description: None,
            agent: Some("claude".to_string()),
            cwd: None,
            role: None,
            creator_pane_id: None,
            enabled: None,
            max_history: None,
            oneshot: None,
        }
    }

    #[test]
    fn test_cron_crud() {
        let mut store = temp_store();

        // Create
        let job = store.create(make_params("Test Cron", "ws-1", "*/30 * * * *")).unwrap();
        assert_eq!(job.name, "Test Cron");
        assert_eq!(job.agent, "claude");
        assert!(job.enabled);
        assert!(job.next_run.is_some());

        // Get
        let fetched = store.get(&job.id).unwrap();
        assert_eq!(fetched.name, "Test Cron");

        // Update
        let updated = store
            .update(
                &job.id,
                CronUpdateParams {
                    name: Some("Renamed".to_string()),
                    description: None,
                    schedule: None,
                    task: None,
                    cwd: None,
                    role: None,
                    enabled: None,
                    max_history: None,
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Renamed");

        // Delete
        store.delete(&job.id).unwrap();
        assert!(store.get(&job.id).is_none());
    }

    #[test]
    fn test_list_for_workspace() {
        let mut store = temp_store();
        store.create(make_params("Job A", "ws-1", "0 * * * *")).unwrap();
        store.create(make_params("Job B", "ws-2", "0 * * * *")).unwrap();
        store.create(make_params("Job C", "ws-1", "0 * * * *")).unwrap();

        assert_eq!(store.list_for_workspace("ws-1").len(), 2);
        assert_eq!(store.list_for_workspace("ws-2").len(), 1);
        assert_eq!(store.list_for_workspace("ws-3").len(), 0);
    }

    #[test]
    fn test_record_run_history() {
        let mut store = temp_store();
        let job = store
            .create(CronCreateParams {
                max_history: Some(3),
                ..make_params("Job", "ws-1", "0 * * * *")
            })
            .unwrap();

        for i in 0..5 {
            store
                .record_run(
                    &job.id,
                    CronRunInfo {
                        run_id: format!("run-{}", i),
                        started_at: 1000 + i,
                        finished_at: Some(2000 + i),
                        status: CronRunStatus::Success,
                        pane_id: None,
                        tab_id: None,
                    },
                )
                .unwrap();
        }

        let history = store.get_history(&job.id);
        assert_eq!(history.len(), 3); // max_history eviction
        assert_eq!(history[0].run_id, "run-2"); // oldest kept
        assert_eq!(history[2].run_id, "run-4"); // newest

        // last_run updated
        let j = store.get(&job.id).unwrap();
        assert_eq!(j.last_run.as_ref().unwrap().run_id, "run-4");
    }

    #[test]
    fn test_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");

        // Create and save
        {
            let mut store = CronStore::new_with_path(path.clone());
            store.create(make_params("Persistent", "ws-1", "0 9 * * *")).unwrap();
        }

        // Load from disk
        {
            let store = CronStore::new_with_path(path);
            assert_eq!(store.list().len(), 1);
            assert_eq!(store.list()[0].name, "Persistent");
        }
    }

    #[test]
    fn test_next_run_computation() {
        let next = compute_next_run("*/30 * * * *").unwrap();
        assert!(next.is_some());
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        // Next run should be within 30 minutes
        assert!(next.unwrap() > now);
        assert!(next.unwrap() - now <= 30 * 60 * 1000 + 1000); // +1s tolerance
    }

    #[test]
    fn test_5field_to_6field() {
        assert_eq!(to_6field("*/5 * * * *"), "0 */5 * * * *");
        assert_eq!(to_6field("0 9 * * 1"), "0 0 9 * * 1");
        // Already 6-field
        assert_eq!(to_6field("30 */5 * * * *"), "30 */5 * * * *");
    }

    #[test]
    fn test_invalid_schedule() {
        assert!(validate_schedule("not a cron").is_err());
        assert!(validate_schedule("* * *").is_err());
        assert!(validate_schedule("0 9 * * *").is_ok());
    }

    #[test]
    fn test_parse_oneshot_relative() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        let ts = parse_oneshot_schedule("in 5m").unwrap();
        assert!(ts > now);
        assert!(ts - now >= 4 * 60_000); // at least 4 min
        assert!(ts - now <= 6 * 60_000); // at most 6 min

        let ts2 = parse_oneshot_schedule("in 1h30m").unwrap();
        assert!(ts2 - now >= 89 * 60_000);
        assert!(ts2 - now <= 91 * 60_000);

        let ts3 = parse_oneshot_schedule("in 2d").unwrap();
        assert!(ts3 - now >= 47 * 3_600_000);
    }

    #[test]
    fn test_parse_oneshot_iso8601() {
        // Future date
        let future = chrono::Utc::now() + chrono::Duration::hours(2);
        let iso = future.to_rfc3339();
        let ts = parse_oneshot_schedule(&iso).unwrap();
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        assert!(ts > now);
    }

    #[test]
    fn test_parse_oneshot_invalid() {
        assert!(parse_oneshot_schedule("not a time").is_err());
        assert!(parse_oneshot_schedule("in 0m").is_err());
    }

    #[test]
    fn test_oneshot_create_and_record_run() {
        let mut store = temp_store();

        let job = store.create(CronCreateParams {
            oneshot: Some(true),
            ..make_params("Once", "ws-1", "in 10m")
        }).unwrap();

        assert!(job.oneshot);
        assert!(job.enabled);
        assert!(job.next_run.is_some());

        // After recording run, oneshot should disable and clear next_run
        store.record_run(&job.id, CronRunInfo {
            run_id: "run-1".into(),
            started_at: 1000,
            finished_at: Some(2000),
            status: CronRunStatus::Success,
            pane_id: None,
            tab_id: None,
        }).unwrap();

        let j = store.get(&job.id).unwrap();
        assert!(!j.enabled);
        assert!(j.next_run.is_none());
    }
}
