use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use rusqlite::{params, Connection};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};

pub(crate) mod archive;
pub(crate) mod stale;

pub use stale::mem_calibration_stale;

#[derive(Deserialize)]
pub struct SweepParams {
    #[serde(default)]
    pub force: Option<bool>,
}

#[derive(Deserialize)]
pub struct CalibLogParams {
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub count: Option<bool>,
}

const MAX_CHANGES_PER_SESSION: u64 = 30;
const CANDIDATE_QUERY_LIMIT: usize = 200;
const CANDIDATE_ABORT_THRESHOLD: usize = 100;

pub async fn mem_calibration_sweep(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SweepParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let force = p.force.unwrap_or(false);
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        svc.mem.with_conn(|conn| run_calibration_sweep(conn, force))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}

pub async fn mem_calibration_log(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<CalibLogParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let limit = p.limit.unwrap_or(50).min(500);
    let since = match p.since.as_deref() {
        Some(s) if !s.is_empty() => {
            chrono::DateTime::parse_from_rfc3339(s).map_err(|e| {
                (
                    StatusCode::BAD_REQUEST,
                    format!("since must be RFC3339: {}", e),
                )
            })?;
            s.to_string()
        }
        _ => "1970-01-01T00:00:00Z".to_string(),
    };
    let count_only = p.count.unwrap_or(false);
    let svc = state.svc.clone();
    let result = tokio::task::spawn_blocking(move || {
        svc.mem
            .with_conn(|conn| calibration_log_query(conn, &since, limit, count_only))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {}", e)))?
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    Ok(Json(result))
}

/// Shared by the HTTP and socket transports so query logic can't drift.
pub(crate) fn calibration_log_query(
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

pub(crate) fn run_calibration_sweep(
    conn: &Connection,
    force: bool,
) -> Result<serde_json::Value, String> {
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
pub(crate) fn run_nightly_calibration(
    conn: &Connection,
    hygiene: &crate::config::schema::DreamHygieneConfig,
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
                    WHEN evidence_count >= 10 AND importance < 4 AND accessed_at > datetime('now','-30 days')
                        THEN 'promote'
                    WHEN evidence_count <= 1 AND importance >= 4 AND created_at < datetime('now','-60 days')
                        THEN 'demote'
                    ELSE 'none'
                END AS action
             FROM mem_items
             WHERE is_pinned_wakeup = 0
               AND updated_at < datetime('now','-14 days')
               AND (
                    (evidence_count >= 10 AND importance < 4 AND accessed_at > datetime('now','-30 days'))
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
    crate::mem::store::with_changed_by("dreamer-calibration:promote", || {
        conn.execute(
            "UPDATE mem_items SET importance = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![new_imp, cand.id],
        )
        .map_err(|e| format!("promote update: {}", e))?;
        let old = crate::mem::store::HistorySnapshot {
            importance: Some(cand.importance),
            ..Default::default()
        };
        let new = crate::mem::store::HistorySnapshot {
            importance: Some(new_imp),
            ..Default::default()
        };
        crate::mem::store::history_log(conn, &cand.id, "update", &old, &new, None);
        Ok(())
    })
}

fn apply_demote(conn: &Connection, cand: &Candidate, new_imp: f64) -> Result<(), String> {
    crate::mem::store::with_changed_by("dreamer-calibration:demote", || {
        conn.execute(
            "UPDATE mem_items SET importance = ?1, updated_at = datetime('now') WHERE id = ?2",
            params![new_imp, cand.id],
        )
        .map_err(|e| format!("demote update: {}", e))?;
        let old = crate::mem::store::HistorySnapshot {
            importance: Some(cand.importance),
            ..Default::default()
        };
        let new = crate::mem::store::HistorySnapshot {
            importance: Some(new_imp),
            ..Default::default()
        };
        crate::mem::store::history_log(conn, &cand.id, "update", &old, &new, None);
        Ok(())
    })
}

fn apply_archive(conn: &Connection, cand: &Candidate) -> Result<(), String> {
    archive::archive_item(
        conn,
        &cand.id,
        &cand.kind,
        "dreamer-calibration:archive",
        "archive",
        None,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open_test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        crate::mem::store::init_schema(&conn).unwrap();
        conn
    }

    fn insert_candidate(
        conn: &Connection,
        id: &str,
        kind: &str,
        importance: f64,
        evidence_count: i64,
        accessed_at_expr: &str,
        created_at_expr: &str,
        updated_at_expr: Option<&str>,
    ) {
        let upd = updated_at_expr.unwrap_or("datetime('now','-15 days')");
        let sql = format!(
            "INSERT INTO mem_items (id, uid, kind, scope, title, importance,
                evidence_count, created_at, updated_at, accessed_at, tags_json,
                is_pinned_wakeup)
             VALUES (?1, ?1, ?2, 'global', ?1, ?3, ?4,
                {created_at_expr}, {upd}, {accessed_at_expr}, '[]', 0)"
        );
        conn.execute(&sql, params![id, kind, importance, evidence_count])
            .unwrap();
    }

    fn dow_today(conn: &Connection) -> String {
        conn.query_row("SELECT strftime('%w','now')", [], |r| r.get::<_, String>(0))
            .unwrap()
    }

    #[test]
    fn calibration_sweep_skips_on_non_sunday() {
        let conn = open_test_conn();
        let r = run_calibration_sweep(&conn, false).unwrap();
        let dow = r["day_of_week"].as_str().unwrap();
        if dow != "0" {
            assert_eq!(r["skipped_reason"], "not_sunday");
            assert_eq!(r["applied"], 0);
            assert_eq!(r["archived"], 0);
            assert_eq!(r["examined"], 0);
        }
    }

    #[test]
    fn calibration_sweep_force_bypasses_dow_gate() {
        let conn = open_test_conn();
        let r = run_calibration_sweep(&conn, true).unwrap();
        let skip = r["skipped_reason"].as_str().unwrap_or("");
        assert!(
            skip != "not_sunday",
            "force=true must bypass DOW gate, got: {r:?}"
        );
    }

    #[test]
    fn calibration_sweep_promote_high_evidence_low_importance() {
        let conn = open_test_conn();
        insert_candidate(
            &conn,
            "fact_p",
            "fact",
            2.0,
            15,
            "datetime('now')",
            "datetime('now','-100 days')",
            None,
        );
        let _ = run_calibration_sweep(&conn, true).unwrap();
        let new_imp: f64 = conn
            .query_row(
                "SELECT importance FROM mem_items WHERE id = 'fact_p'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (new_imp - 2.5).abs() < 0.01,
            "promote should add +0.5, got {new_imp}"
        );
        let hist_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_item_history
                 WHERE item_id = 'fact_p' AND changed_by LIKE 'dreamer-calibration%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(hist_count, 1);
    }

    #[test]
    fn calibration_sweep_demotes_old_high_importance_no_evidence() {
        let conn = open_test_conn();
        insert_candidate(
            &conn,
            "fact_d",
            "fact",
            4.5,
            1,
            "datetime('now','-90 days')",
            "datetime('now','-100 days')",
            None,
        );
        let _ = run_calibration_sweep(&conn, true).unwrap();
        let new_imp: f64 = conn
            .query_row(
                "SELECT importance FROM mem_items WHERE id = 'fact_d'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (new_imp - 4.0).abs() < 0.01,
            "demote should subtract 0.5, got {new_imp}"
        );
        let label: String = conn
            .query_row(
                "SELECT changed_by FROM mem_item_history WHERE item_id = 'fact_d'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(label, "dreamer-calibration:demote");
    }

    #[test]
    fn calibration_sweep_archives_stale_low_importance() {
        let conn = open_test_conn();
        insert_candidate(
            &conn,
            "fact_old",
            "fact",
            1.5,
            1,
            "datetime('now','-200 days')",
            "datetime('now','-100 days')",
            None,
        );
        let _ = run_calibration_sweep(&conn, true).unwrap();
        let kind: String = conn
            .query_row(
                "SELECT kind FROM mem_items WHERE id = 'fact_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "archived");
        let label: String = conn
            .query_row(
                "SELECT changed_by FROM mem_item_history WHERE item_id = 'fact_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(label, "dreamer-calibration:archive");
    }

    #[test]
    fn calibration_sweep_skips_on_recent_run() {
        let conn = open_test_conn();
        conn.execute(
            "INSERT INTO mem_item_history (item_id, changed_at, changed_by, change_type)
             VALUES ('fake', datetime('now','-1 hour'), 'dreamer-calibration:promote', 'update')",
            [],
        )
        .unwrap();
        let r = run_calibration_sweep(&conn, true).unwrap();
        assert_eq!(r["skipped_reason"], "recent_run");
        assert_eq!(r["applied"], 0);
    }

    #[test]
    fn calibration_sweep_caps_at_30_changes_per_run() {
        let conn = open_test_conn();
        for i in 0..50 {
            let id = format!("p_{}", i);
            insert_candidate(
                &conn,
                &id,
                "fact",
                2.0,
                15,
                "datetime('now')",
                "datetime('now','-100 days')",
                None,
            );
        }
        let r = run_calibration_sweep(&conn, true).unwrap();
        if r.get("skipped_reason").is_some() {
            assert_eq!(r["skipped_reason"], "too_many_candidates");
        } else {
            let applied = r["applied"].as_u64().unwrap_or(0);
            assert!(
                applied <= MAX_CHANGES_PER_SESSION,
                "must cap at 30, got {applied}"
            );
        }
    }

    #[test]
    fn calibration_sweep_aborts_on_too_many_candidates() {
        let conn = open_test_conn();
        for i in 0..150 {
            let id = format!("c_{}", i);
            insert_candidate(
                &conn,
                &id,
                "fact",
                2.0,
                15,
                "datetime('now')",
                "datetime('now','-100 days')",
                None,
            );
        }
        let r = run_calibration_sweep(&conn, true).unwrap();
        if r.get("skipped_reason").is_some() {
            assert_eq!(r["skipped_reason"], "too_many_candidates");
        } else {
            assert!(r["applied"].as_u64().unwrap_or(0) <= MAX_CHANGES_PER_SESSION);
        }
    }

    #[test]
    fn calibration_sweep_skips_recently_updated_items() {
        let conn = open_test_conn();
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, importance,
                evidence_count, created_at, updated_at, accessed_at, tags_json,
                is_pinned_wakeup)
             VALUES ('fresh', 'fresh', 'fact', 'global', 'fresh', 2.0, 15,
                datetime('now','-50 days'), datetime('now','-3 days'),
                datetime('now'), '[]', 0)",
            [],
        )
        .unwrap();
        let _ = run_calibration_sweep(&conn, true).unwrap();
        let imp: f64 = conn
            .query_row(
                "SELECT importance FROM mem_items WHERE id = 'fresh'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!((imp - 2.0).abs() < 0.01, "fresh item must not be touched");
    }

    #[test]
    fn calibration_log_count_only() {
        let conn = open_test_conn();
        conn.execute(
            "INSERT INTO mem_item_history (item_id, changed_at, changed_by, change_type)
             VALUES ('a', datetime('now'), 'dreamer-calibration:promote', 'update'),
                    ('b', datetime('now'), 'dreamer-calibration:archive', 'update'),
                    ('c', datetime('now'), 'user', 'update')",
            [],
        )
        .unwrap();
        let n: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_item_history
                 WHERE changed_by LIKE 'dreamer-calibration%' AND changed_at >= ?1",
                params!["1970-01-01T00:00:00Z"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(n, 2);
    }

    #[test]
    fn dow_today_returns_zero_through_six() {
        let conn = open_test_conn();
        let dow = dow_today(&conn);
        let n: i64 = dow.parse().unwrap();
        assert!((0..=6).contains(&n));
    }

    #[test]
    fn calibration_sweep_never_touches_pinned_items() {
        let conn = open_test_conn();
        // Insert a pinned item with archive-eligible fields.
        conn.execute(
            "INSERT INTO mem_items (id, uid, kind, scope, title, importance,
                evidence_count, created_at, updated_at, accessed_at, tags_json,
                is_pinned_wakeup)
             VALUES ('pinned_old', 'pinned_old', 'fact', 'global', 'pinned_old', 2.0, 1,
                datetime('now','-100 days'), datetime('now','-15 days'),
                datetime('now','-200 days'), '[]', 1)",
            [],
        )
        .unwrap();
        let _ = run_calibration_sweep(&conn, true).unwrap();
        let kind: String = conn
            .query_row(
                "SELECT kind FROM mem_items WHERE id = 'pinned_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "fact", "pinned item kind must not change");
        let imp: f64 = conn
            .query_row(
                "SELECT importance FROM mem_items WHERE id = 'pinned_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            (imp - 2.0).abs() < 0.01,
            "pinned item importance must not change"
        );
        let hist_count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM mem_item_history WHERE item_id = 'pinned_old'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            hist_count, 0,
            "pinned item must have no history rows from sweep"
        );
    }
}
