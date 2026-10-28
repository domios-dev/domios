use rusqlite::{params, Connection};

pub mod archive;
pub mod stale;

const MAX_CHANGES_PER_SESSION: u64 = 30;
const CANDIDATE_QUERY_LIMIT: usize = 200;
const CANDIDATE_ABORT_THRESHOLD: usize = 100;

/// Shared by the HTTP and socket transports so query logic can't drift.
pub fn calibration_log_query(
    conn: &Connection,
    since: &str,
    limit: usize,
    count_only: bool,
) -> Result<serde_json::Value, String> {
    if count_only {
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_item_history
                 WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at >= ?1",
                params![since],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        return Ok(serde_json::json!({ "count": n }));
    }
    let mut stmt = conn
        .prepare(
            "SELECT item_id, changed_at, changed_by, change_type,
                old_importance, new_importance, old_kind, new_kind, reason
             FROM mem_item_history
             WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at >= ?1
             ORDER BY changed_at DESC LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![since, limit as i64], |r| {
            Ok(serde_json::json!({
                "item_id": r.get::<_, String>(0)?,
                "changed_at": r.get::<_, String>(1)?,
                "changed_by": r.get::<_, String>(2)?,
                "change_type": r.get::<_, String>(3)?,
                "old_importance": r.get::<_, Option<f64>>(4)?,
                "new_importance": r.get::<_, Option<f64>>(5)?,
                "old_kind": r.get::<_, Option<String>>(6)?,
                "new_kind": r.get::<_, Option<String>>(7)?,
                "reason": r.get::<_, Option<String>>(8)?,
            }))
        })
        .map_err(|e| e.to_string())?;
    let entries: Vec<_> = rows.filter_map(|r| r.ok()).collect();
    Ok(serde_json::json!({ "entries": entries }))
}

#[derive(Debug, Clone)]
struct Candidate {
    id: String,
    kind: String,
    importance: f64,
    action: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum CalibAction {
    Promote(f64),
    Demote(f64),
    Archive,
    None,
}

pub fn run_calibration_sweep(conn: &Connection, force: bool) -> Result<serde_json::Value, String> {
    let dow: String = conn
        .query_row("SELECT strftime('%w','now')", [], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    if !force && dow != "0" {
        return Ok(serde_json::json!({
            "day_of_week": dow,
            "examined": 0,
            "applied": 0,
            "archived": 0,
            "skipped_reason": "not_sunday",
        }));
    }

    let recent: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM mem_item_history
             WHERE changed_by LIKE 'dreamer-calibration%' AND changed_by != ?1
               AND changed_at >= datetime('now','-6 hours')",
            params![stale::STALE_ACTOR],
            |r| r.get(0),
        )
        .map_err(|e| e.to_string())?;
    if recent > 0 {
        return Ok(serde_json::json!({
            "day_of_week": dow,
            "examined": 0,
            "applied": 0,
            "archived": 0,
            "skipped_reason": "recent_run",
        }));
    }

    let candidates = load_candidates(conn)?;

    if candidates.len() > CANDIDATE_ABORT_THRESHOLD {
        return Ok(serde_json::json!({
            "day_of_week": dow,
            "examined": 0,
            "applied": 0,
            "archived": 0,
            "skipped_reason": "too_many_candidates",
            "candidate_count": candidates.len(),
        }));
    }

    let mut applied: u64 = 0;
    let mut archived: u64 = 0;
    let mut archived_ids: Vec<String> = Vec::new();
    let mut examined: u64 = 0;
    let mut total_changes: u64 = 0;

    // Transactional: partial state poisons the recent_run re-entry guard on crash.
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    let sweep_result = (|| -> Result<(), String> {
        for cand in candidates.iter() {
            if total_changes >= MAX_CHANGES_PER_SESSION {
                break;
            }
            examined += 1;
            match classify_calibration(cand) {
                CalibAction::Promote(new_imp) => {
                    apply_promote(conn, cand, new_imp)?;
                    applied += 1;
                    total_changes += 1;
                }
                CalibAction::Demote(new_imp) => {
                    apply_demote(conn, cand, new_imp)?;
                    applied += 1;
                    total_changes += 1;
                }
                CalibAction::Archive => {
                    apply_archive(conn, cand)?;
                    archived_ids.push(cand.id.clone());
                    archived += 1;
                    total_changes += 1;
                }
                CalibAction::None => {}
            }
        }
        Ok(())
    })();
    match sweep_result {
        Ok(()) => conn.execute_batch("COMMIT").map_err(|e| e.to_string())?,
        Err(e) => {
            conn.execute_batch("ROLLBACK").ok();
            return Err(e);
        }
    }

    Ok(serde_json::json!({
        "day_of_week": dow,
        "examined": examined,
        "applied": applied,
        "archived": archived,
        "archived_ids": archived_ids,
    }))
}

/// Nightly path: the weekly sweep keeps its Sunday gate, while the stale
/// archive runs every night, but only when `memory.dream_hygiene` enables it.
pub fn run_nightly_calibration(
    conn: &Connection,
    hygiene: &codeterm_types::config::DreamHygieneConfig,
    now: chrono::DateTime<chrono::Utc>,
) -> Result<serde_json::Value, String> {
    let mut result = run_calibration_sweep(conn, false)?;
    let stale = stale::run_stale_archive(conn, hygiene, now, stale::StaleMode::Apply, None)?;
    if let Some(obj) = result.as_object_mut() {
        obj.insert("stale".into(), stale);
    }
    Ok(result)
}

fn load_candidates(conn: &Connection) -> Result<Vec<Candidate>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, kind, importance,
                CASE
                    WHEN evidence_count = 1 AND importance <= 2 AND created_at < datetime('now','-90 days') AND kind IN ('fact','note')
                        THEN 'archive'
                    WHEN author_kind != 'neighbor' AND evidence_count >= 10 AND importance < 4 AND accessed_at > datetime('now','-30 days')
                        THEN 'promote'
                    WHEN evidence_count <= 1 AND importance >= 4 AND created_at < datetime('now','-60 days')
                        THEN 'demote'
                    ELSE 'none'
                END AS action
             FROM mem_items
             WHERE is_pinned_wakeup = 0
               AND updated_at < datetime('now','-14 days')
               AND (
                    (author_kind != 'neighbor' AND evidence_count >= 10 AND importance < 4 AND accessed_at > datetime('now','-30 days'))
                 OR (evidence_count <= 1 AND importance >= 4 AND created_at < datetime('now','-60 days'))
                 OR (evidence_count = 1 AND importance <= 2 AND created_at < datetime('now','-90 days') AND kind IN ('fact','note'))
               )
             LIMIT ?1",
        )
        .map_err(|e| format!("prepare candidates: {}", e))?;
    let rows = stmt
        .query_map(params![CANDIDATE_QUERY_LIMIT as i64], |r| {
            Ok(Candidate {
                id: r.get(0)?,
                kind: r.get(1)?,
                importance: r.get(2)?,
                action: r.get(3)?,
            })
        })
        .map_err(|e| format!("query candidates: {}", e))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| format!("row candidate: {}", e))?);
    }
    Ok(out)
}

fn classify_calibration(c: &Candidate) -> CalibAction {
    match c.action.as_str() {
        "promote" => CalibAction::Promote((c.importance + 0.5).min(5.0)),
        "demote" => CalibAction::Demote((c.importance - 0.5).max(0.0)),
        "archive" => CalibAction::Archive,
        _ => CalibAction::None,
    }
}

fn apply_promote(conn: &Connection, cand: &Candidate, new_imp: f64) -> Result<(), String> {
    crate::store::refuse_neighbor_promotion(conn, &cand.id)?;
    crate::store::with_system_author("dreamer-calibration:promote", || {
        conn.execute(
            "UPDATE mem_items SET importance = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![new_imp, cand.id],
        )
        .map_err(|e| format!("promote update: {}", e))?;
        let old = crate::store::HistorySnapshot {
            importance: Some(cand.importance),
            ..Default::default()
        };
        let new = crate::store::HistorySnapshot {
            importance: Some(new_imp),
            ..Default::default()
        };
        crate::store::history_log(conn, &cand.id, "update", &old, &new, None);
        Ok(())
    })
}

fn apply_demote(conn: &Connection, cand: &Candidate, new_imp: f64) -> Result<(), String> {
    crate::store::with_system_author("dreamer-calibration:demote", || {
        conn.execute(
            "UPDATE mem_items SET importance = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![new_imp, cand.id],
        )
        .map_err(|e| format!("demote update: {}", e))?;
        let old = crate::store::HistorySnapshot {
            importance: Some(cand.importance),
            ..Default::default()
        };
        let new = crate::store::HistorySnapshot {
            importance: Some(new_imp),
            ..Default::default()
        };
        crate::store::history_log(conn, &cand.id, "update", &old, &new, None);
        Ok(())
    })
}

fn apply_archive(conn: &Connection, cand: &Candidate) -> Result<(), String> {
    crate::store::with_default_system_author("calibration", || {
        archive::archive_item(
            conn,
            &cand.id,
            &cand.kind,
            "dreamer-calibration:archive",
            "archive",
            None,
        )
    })
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod organizer_guard;
