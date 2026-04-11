//! Dreaming consolidation — promotes high-signal mem_items based on access
//! frequency and importance. Light + Deep phases run as a cron-spawned agent
//! by default; can also be invoked manually via `mem dream now`.
//!
//! In Phase 3.4 dreaming runs locally (no LLM call) — pure SQL signals over
//! the existing `accessed_at` and `importance` columns. The full vision in
//! mem-architecture.md (Light → Deep → REM with token budgets) is layered on
//! top of this baseline in later phases as needed.

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::MemManager;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamReport {
    pub considered: u64,
    pub promoted: u64,
    pub dropped: u64,
    pub phase: String,
}

/// Light phase: identify learnings that have been accessed multiple times,
/// auto-promote them to facts and bump importance.
///
/// Weighted scoring for learning promotion (inspired by OpenClaw).
///
/// Score = frequency × 0.30 + recency × 0.25 + importance × 0.25 + age × 0.20
///
/// - frequency: access_count / 10 (capped at 1.0)
/// - recency: exp(-days_since_access * ln2 / 14) — 14-day half-life
/// - importance: current_importance / 5.0
/// - age: min(days_since_creation / 7, 1.0) — older = more validated
///
/// Items with score >= 0.5 AND access_count >= 3 get promoted.
pub fn light_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let conn = mem.conn_for_dream();
    let conn = conn.lock();

    let mut considered = 0u64;
    let mut promoted = 0u64;
    let now_ts = chrono::Utc::now();

    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            r#"SELECT id, access_count, importance, created_at, accessed_at
               FROM mem_items
               WHERE kind = 'learning'
                 AND access_count >= 3
                 AND created_at < ?1"#,
        )
        .map_err(|e| e.to_string())?;

    let candidates: Vec<(String, i64, f64, String, Option<String>)> = stmt
        .query_map(params![one_day_ago], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    for (id, access_count, importance, created_at, accessed_at) in &candidates {
        considered += 1;

        let frequency = (*access_count as f64 / 10.0).min(1.0);

        let recency = accessed_at
            .as_ref()
            .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
            .map(|a| {
                let days = (now_ts - a.with_timezone(&chrono::Utc))
                    .num_hours()
                    .max(0) as f64 / 24.0;
                (-days * 0.693 / 14.0).exp() // ln(2)/14 ≈ 0.0495
            })
            .unwrap_or(0.0);

        let imp_score = (*importance / 5.0).min(1.0);

        let age = chrono::DateTime::parse_from_rfc3339(created_at)
            .ok()
            .map(|c| {
                let days = (now_ts - c.with_timezone(&chrono::Utc))
                    .num_hours()
                    .max(0) as f64 / 24.0;
                (days / 7.0).min(1.0)
            })
            .unwrap_or(0.5);

        let score = frequency * 0.30 + recency * 0.25 + imp_score * 0.25 + age * 0.20;

        if score >= 0.5 {
            let now = chrono::Utc::now().to_rfc3339();
            let n = conn
                .execute(
                    "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
                    params![&now, &id],
                )
                .map_err(|e| e.to_string())?;
            if n > 0 {
                promoted += 1;
            }
        }
    }

    Ok(DreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
    })
}

/// Deep phase: expire stale conversations (>30 days, never accessed, low importance).
/// Sets importance to 0 instead of deleting — items remain searchable but won't
/// appear in L1 wake-up. Keeps history intact for future reference.
pub fn deep_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let conn = mem.conn_for_dream();
    let conn = conn.lock();

    let one_month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let now = chrono::Utc::now().to_rfc3339();
    let expired = conn
        .execute(
            r#"UPDATE mem_items
               SET importance = 0.0, updated_at = ?1
               WHERE kind = 'conversation'
                 AND access_count = 0
                 AND created_at < ?2
                 AND importance > 0.0
                 AND importance < 4.0"#,
            params![&now, &one_month_ago],
        )
        .map_err(|e| e.to_string())? as u64;

    Ok(DreamReport {
        considered: expired,
        promoted: 0,
        dropped: expired,
        phase: "deep".into(),
    })
}

/// Prepare data for the REM agent — returns a text summary of candidates.
/// Called by `codeterm mem dream --report` so the agent gets concrete data.
pub fn prepare_rem_data(mem: &MemManager) -> Result<String, String> {
    let conn = mem.conn_for_dream();
    let conn = conn.lock();
    let mut lines = Vec::new();

    // 1. Unclassified items (no area)
    let mut stmt = conn.prepare(
        "SELECT id, kind, title FROM mem_items WHERE (area IS NULL OR area = '') LIMIT 20"
    ).map_err(|e| e.to_string())?;
    let unclassified: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !unclassified.is_empty() {
        lines.push(format!("UNCLASSIFIED ({}):", unclassified.len()));
        for (id, kind, title) in &unclassified {
            lines.push(format!("  {} [{}] {}", id, kind, title));
        }
    }

    // 2. Potential duplicates — same kind, similar titles (first 6 chars match)
    let mut stmt = conn.prepare(
        r#"SELECT a.id, b.id, a.kind, a.title, b.title, a.importance, b.importance
           FROM mem_items a
           JOIN mem_items b ON a.kind = b.kind
             AND a.id < b.id
             AND substr(a.title, 1, 12) = substr(b.title, 1, 12)
             AND a.title != b.title
           LIMIT 20"#
    ).map_err(|e| e.to_string())?;
    let dupes: Vec<(String, String, String, String, String, f64, f64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?, r.get(6)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !dupes.is_empty() {
        lines.push(format!("\nPOTENTIAL DUPLICATES ({}):", dupes.len()));
        for (a_id, b_id, kind, a_title, b_title, a_imp, b_imp) in &dupes {
            lines.push(format!("  [{}] \"{}\" (imp={}) vs \"{}\" (imp={})", kind, a_title, a_imp, b_title, b_imp));
            lines.push(format!("    ids: {} vs {}", a_id, b_id));
        }
    }

    // 3. Recent high-access items (last 7 days, for pattern finding)
    let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let mut stmt = conn.prepare(
        r#"SELECT id, kind, title, area, access_count, recurrence_count
           FROM mem_items
           WHERE updated_at > ?1 AND access_count >= 2
           ORDER BY access_count DESC
           LIMIT 15"#
    ).map_err(|e| e.to_string())?;
    let hot: Vec<(String, String, String, Option<String>, i64, i64)> = stmt
        .query_map(params![&week_ago], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !hot.is_empty() {
        lines.push(format!("\nHOT ITEMS last 7d ({}):", hot.len()));
        for (id, kind, title, area, ac, rc) in &hot {
            let recur = if *rc > 0 { format!(" recur={}", rc) } else { String::new() };
            lines.push(format!("  {} [{}] area={} access={}{} {}", id, kind, area.as_deref().unwrap_or("?"), ac, recur, title));
        }
    }

    // 4. Area distribution
    let mut stmt = conn.prepare(
        "SELECT COALESCE(area, '(none)'), COUNT(*) FROM mem_items GROUP BY area ORDER BY COUNT(*) DESC LIMIT 10"
    ).map_err(|e| e.to_string())?;
    let areas: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    lines.push("\nAREA DISTRIBUTION:".into());
    for (area, count) in &areas {
        lines.push(format!("  {}: {} items", area, count));
    }

    if lines.is_empty() {
        Ok("Nothing to optimize.".into())
    } else {
        Ok(lines.join("\n"))
    }
}

/// Run SQL phases (light + deep), then spawn headless REM worker.
pub fn run_all(mem: &MemManager) -> Result<Vec<DreamReport>, String> {
    let mut out = Vec::new();
    out.push(light_phase(mem)?);
    out.push(deep_phase(mem)?);
    Ok(out)
}

/// Spawn a headless dreaming worker for REM phase (dedup, classify, insights, diary).
/// Called after `run_all` completes SQL phases.
pub fn spawn_rem_worker(svc: &std::sync::Arc<crate::service::ServiceContext>) {
    let mem_cfg = crate::config::memory_config();
    let agent = if mem_cfg.dreaming_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.dreaming_agent.clone()
    };
    let model = mem_cfg.dreaming_model.clone();

    match super::workers::spawn_headless(
        svc,
        "dreaming:rem".into(),
        crate::prompts::DREAMING_REM.to_string(),
        "system:dreaming".into(),
        agent,
        model,
    ) {
        Ok(wid) => log::info!("[dreaming] REM worker spawned: {}", wid),
        Err(e) => log::error!("[dreaming] REM worker spawn failed: {}", e),
    }
}

/// Marker name for the auto-installed dreaming cron job. Used to
/// detect "already exists" so we don't create duplicates on every start.
pub const DREAMING_CRON_NAME: &str = "mem-dreaming-auto";

/// The nightly optimizer task. First runs SQL phases (light + deep),
/// then spawns a graph worker for intelligent optimization.
const DREAMING_TASK: &str = "codeterm mem dream && codeterm agent report \"dreaming complete\"\n";

/// Create the bundled dreaming cron if it doesn't exist already.
/// - Schedule: "0 3 * * *" (3 AM)
/// - Jitter: ±90 minutes (5_400_000 ms) — anti-anti-cheat, so the job
///   actually fires anywhere from 1:30 AM to 4:30 AM, never the same
///   instant two days in a row.
/// - Task: runs `codeterm mem dream` in a fresh agent
/// - Workspace: "general" (the GA workspace)
///
/// Returns true if a new cron was created, false if one already existed.
pub fn ensure_dreaming_cron(cron_store: &mut crate::cron::store::CronStore) -> Result<bool, String> {
    let mem_cfg = crate::config::memory_config();

    // Update agent on existing cron if config changed
    if let Some(job) = cron_store.list().iter().find(|j| j.name == DREAMING_CRON_NAME) {
        let desired = if mem_cfg.dreaming_agent.is_empty() {
            crate::config::default_agent_provider()
        } else {
            mem_cfg.dreaming_agent.clone()
        };
        if job.agent != desired {
            let job_id = job.id.clone();
            if let Some(j) = cron_store.get_mut(&job_id) {
                j.agent = desired;
                let _ = cron_store.save();
            }
        }
        return Ok(false);
    }

    let agent = if mem_cfg.dreaming_agent.is_empty() {
        None
    } else {
        Some(mem_cfg.dreaming_agent.clone())
    };

    let model = if mem_cfg.dreaming_model.is_empty() {
        None
    } else {
        Some(mem_cfg.dreaming_model.clone())
    };

    let _ = cron_store.create(crate::cron::store::CronCreateParams {
        name: DREAMING_CRON_NAME.into(),
        workspace_id: String::new(),
        schedule: "0 3 * * *".into(),
        task: DREAMING_TASK.into(),
        description: Some("Memory optimizer. Runs nightly as a graph worker inside Memory Panel. Toggle on/off in settings. Cannot be deleted.".into()),
        agent,
        model,
        cwd: None,
        role: None,
        creator_pane_id: None,
        enabled: Some(true),
        max_history: Some(30),
        oneshot: Some(false),
        jitter_max_ms: Some(5_400_000),
    })?;
    // Mark as protected after creation
    if let Some(job) = cron_store.list().iter().find(|j| j.name == DREAMING_CRON_NAME) {
        let job_id = job.id.clone();
        if let Some(j) = cron_store.get_mut(&job_id) {
            j.protected = true;
        }
        let _ = cron_store.save();
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::{MemKind, MemScope, NewMemItem};

    #[test]
    fn light_phase_promotes_accessed_learnings() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Learning,
                scope: MemScope::Global,
                title: "test".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 3.0,
            })
            .unwrap();

        // Backdate creation + simulate 3 accesses (meets threshold)
        {
            let conn = mem.conn_for_dream();
            let conn = conn.lock();
            let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1, accessed_at = ?2, access_count = 3 WHERE id = ?3",
                params![&two_days_ago, &two_days_ago, &id],
            )
            .unwrap();
        }

        let report = light_phase(&mem).unwrap();
        assert_eq!(report.considered, 1);
        assert_eq!(report.promoted, 1);

        let after = mem.get(&id).unwrap().unwrap();
        assert_eq!(after.kind, "fact");
        assert!(after.importance >= 4.0);
    }

    #[test]
    fn deep_phase_expires_old_unaccessed_conversations() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Conversation,
                scope: MemScope::Global,
                title: "old chat".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "lorem ipsum".into(),
                source: None,
                importance: 2.0,
            })
            .unwrap();

        // Backdate to 60 days ago, never accessed
        {
            let conn = mem.conn_for_dream();
            let conn = conn.lock();
            let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1, access_count = 0 WHERE id = ?2",
                params![&sixty_days_ago, &id],
            )
            .unwrap();
        }

        let report = deep_phase(&mem).unwrap();
        assert_eq!(report.dropped, 1);
        // Item still exists but with importance = 0 (expired, not deleted)
        let after = mem.get(&id).unwrap().unwrap();
        assert_eq!(after.importance, 0.0);
    }

    #[test]
    fn ensure_dreaming_cron_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        let first = ensure_dreaming_cron(&mut store).unwrap();
        assert!(first, "first call should create");
        let second = ensure_dreaming_cron(&mut store).unwrap();
        assert!(!second, "second call should be no-op");
        let count = store
            .list()
            .iter()
            .filter(|j| j.name == DREAMING_CRON_NAME)
            .count();
        assert_eq!(count, 1, "exactly one auto-cron must exist");

        let job = store.list().iter().find(|j| j.name == DREAMING_CRON_NAME).unwrap();
        assert_eq!(job.jitter_max_ms, Some(5_400_000));
        assert!(job.enabled);
        assert_eq!(job.schedule, "0 3 * * *");
    }

    #[test]
    fn deep_phase_keeps_high_importance_conversations() {
        let mem = MemManager::open_in_memory().unwrap();
        let id = mem
            .save(NewMemItem {
                kind: MemKind::Conversation,
                scope: MemScope::Global,
                title: "important chat".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 4.5,
            })
            .unwrap();
        {
            let conn = mem.conn_for_dream();
            let conn = conn.lock();
            let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1 WHERE id = ?2",
                params![&sixty_days_ago, &id],
            )
            .unwrap();
        }

        let r = deep_phase(&mem).unwrap();
        assert_eq!(r.dropped, 0);
        assert!(mem.get(&id).unwrap().is_some());
    }
}
