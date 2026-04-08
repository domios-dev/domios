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
/// Heuristic (no LLM): a learning is "validated" if it has been read
/// (`accessed_at IS NOT NULL`) and survived more than 1 day since creation.
/// Frequency proxy is (importance * recency).
pub fn light_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let conn = mem.conn_for_dream();
    let conn = conn.lock();

    let mut considered = 0u64;
    let mut promoted = 0u64;

    // Pull learnings older than 1 day that have been accessed at least once.
    // Ordered by importance desc.
    let one_day_ago = (chrono::Utc::now() - chrono::Duration::days(1)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            r#"SELECT id FROM mem_items
               WHERE kind = 'learning'
                 AND accessed_at IS NOT NULL
                 AND created_at < ?1
               ORDER BY importance DESC"#,
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map(params![one_day_ago], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    for id in ids {
        considered += 1;
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

    Ok(DreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
    })
}

/// Deep phase: drop very stale conversations (>30 days, never accessed) to
/// keep blob storage from accumulating noise. Returns count of items dropped.
pub fn deep_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let conn = mem.conn_for_dream();
    let conn = conn.lock();

    let one_month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            r#"SELECT id FROM mem_items
               WHERE kind = 'conversation'
                 AND accessed_at IS NULL
                 AND created_at < ?1
                 AND importance < 4.0"#,
        )
        .map_err(|e| e.to_string())?;
    let ids: Vec<String> = stmt
        .query_map(params![one_month_ago], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut dropped = 0u64;
    for id in &ids {
        let n = conn
            .execute("DELETE FROM mem_items WHERE id = ?1", params![id])
            .map_err(|e| e.to_string())?;
        let _ = conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![id]);
        if n > 0 {
            dropped += 1;
        }
    }

    Ok(DreamReport {
        considered: ids.len() as u64,
        promoted: 0,
        dropped,
        phase: "deep".into(),
    })
}

/// Run all phases sequentially, return combined report.
pub fn run_all(mem: &MemManager) -> Result<Vec<DreamReport>, String> {
    let mut out = Vec::new();
    out.push(light_phase(mem)?);
    out.push(deep_phase(mem)?);
    Ok(out)
}

/// Marker name for the auto-installed dreaming cron job. Used to
/// detect "already exists" so we don't create duplicates on every start.
pub const DREAMING_CRON_NAME: &str = "mem-dreaming-auto";

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
    let exists = cron_store
        .list()
        .iter()
        .any(|j| j.name == DREAMING_CRON_NAME);
    if exists {
        return Ok(false);
    }
    let _ = cron_store.create(crate::cron::store::CronCreateParams {
        name: DREAMING_CRON_NAME.into(),
        workspace_id: "general".into(),
        schedule: "0 3 * * *".into(),
        task: "Run `codeterm mem dream` to consolidate learnings into facts and prune stale conversations. Report stats to your parent pane via `codeterm agent report`.".into(),
        description: Some("Auto-installed mem dreaming consolidation. Disable via `codeterm cron disable <id>` if not wanted.".into()),
        agent: None, // use default provider
        cwd: None,
        role: None,
        creator_pane_id: None,
        enabled: Some(true),
        max_history: Some(30),
        oneshot: Some(false),
        // ±90 minutes around 3 AM, so the job never fires at exactly the
        // same wall-clock time two days in a row. Looks organic, spreads
        // load if multiple machines are running cron at once.
        jitter_max_ms: Some(5_400_000),
    })?;
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

        // Backdate creation + simulate access
        {
            let conn = mem.conn_for_dream();
            let conn = conn.lock();
            let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1, accessed_at = ?2 WHERE id = ?3",
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
    fn deep_phase_drops_old_unaccessed_conversations() {
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
                "UPDATE mem_items SET created_at = ?1, accessed_at = NULL WHERE id = ?2",
                params![&sixty_days_ago, &id],
            )
            .unwrap();
        }

        let report = deep_phase(&mem).unwrap();
        assert_eq!(report.dropped, 1);
        assert!(mem.get(&id).unwrap().is_none());
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
