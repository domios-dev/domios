//! Staleness archive on the calibration sweep: an item untouched for a year,
//! directly or through its graph neighbourhood, is archived, never deleted.

use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, Duration, SecondsFormat, Utc};
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use super::archive::archive_item;
use crate::api::routes::{check_auth, AppState};
use crate::config::schema::DreamHygieneConfig;

pub(crate) const STALE_ACTOR: &str = "dreamer-calibration:stale";
pub(crate) const STALE_AGE_DAYS: i64 = 365;
const MAX_DRY_RUN_LIMIT: usize = 1000;

// ?1 is the cutoff as a Julian day; an unparseable timestamp never reads as stale.
const STALE_PREDICATE: &str = "
    julianday(m.created_at) < ?1
    AND COALESCE(m.is_pinned_wakeup, 0) = 0
    AND m.kind NOT IN ('skill', 'rule', 'preference', 'archived')
    AND m.importance < 4.0
    AND julianday(m.updated_at) < ?1
    AND (m.accessed_at IS NULL OR julianday(m.accessed_at) < ?1)
    AND NOT EXISTS (SELECT 1 FROM mem_item_history h
        WHERE h.item_id = m.id AND julianday(h.changed_at) >= ?1)
    AND NOT EXISTS (SELECT 1 FROM relations r WHERE r.from_id = m.id
        AND (julianday(r.valid_from) >= ?1 OR julianday(r.created_at) >= ?1
             OR julianday(r.valid_to) >= ?1))
    AND NOT EXISTS (SELECT 1 FROM relations r WHERE r.to_id = m.id
        AND (julianday(r.valid_from) >= ?1 OR julianday(r.created_at) >= ?1
             OR julianday(r.valid_to) >= ?1))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN mem_items n ON n.id = r.to_id
        WHERE r.from_id = m.id AND r.valid_to IS NULL
          AND (julianday(n.updated_at) >= ?1 OR julianday(n.accessed_at) >= ?1))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN mem_items n ON n.id = r.from_id
        WHERE r.to_id = m.id AND r.valid_to IS NULL
          AND (julianday(n.updated_at) >= ?1 OR julianday(n.accessed_at) >= ?1))
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN entities e ON e.id = r.to_id
        WHERE r.from_id = m.id AND r.valid_to IS NULL AND julianday(e.updated_at) >= ?1)
    AND NOT EXISTS (SELECT 1 FROM relations r JOIN entities e ON e.id = r.from_id
        WHERE r.to_id = m.id AND r.valid_to IS NULL AND julianday(e.updated_at) >= ?1)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StaleMode {
    DryRun,
    Apply,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct StaleSelection {
    pub id: String,
    pub kind: String,
    pub importance: f64,
    pub created_at: String,
    pub last_touch: String,
    pub active_neighbours: i64,
    pub reason: String,
}

pub(crate) fn julian_day(at: DateTime<Utc>) -> f64 {
    at.timestamp_millis() as f64 / 86_400_000.0 + 2_440_587.5
}

/// Nightly caps count from the start of the current UTC day.
pub(crate) fn utc_day_start(now: DateTime<Utc>) -> DateTime<Utc> {
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .map(|d| d.and_utc())
        .unwrap_or(now)
}

fn stale_cutoff(now: DateTime<Utc>) -> DateTime<Utc> {
    now - Duration::days(STALE_AGE_DAYS)
}

/// Single-item probe of [`STALE_PREDICATE`], the one definition of stale.
#[cfg(test)]
pub(crate) fn is_stale(conn: &Connection, id: &str, now: DateTime<Utc>) -> Result<bool, String> {
    let sql =
        format!("SELECT EXISTS(SELECT 1 FROM mem_items m WHERE m.id = ?2 AND {STALE_PREDICATE})");
    conn.query_row(&sql, params![julian_day(stale_cutoff(now)), id], |r| {
        r.get::<_, bool>(0)
    })
    .map_err(|e| format!("stale predicate: {e}"))
}

fn count_stale(conn: &Connection, cutoff: f64) -> Result<i64, String> {
    let sql = format!("SELECT COUNT(*) FROM mem_items m WHERE {STALE_PREDICATE}");
    conn.query_row(&sql, params![cutoff], |r| r.get(0))
        .map_err(|e| format!("stale count: {e}"))
}

fn select_stale(
    conn: &Connection,
    cutoff: DateTime<Utc>,
    limit: usize,
) -> Result<Vec<StaleSelection>, String> {
    let sql = format!(
        "SELECT m.id, m.kind, m.importance, m.created_at,
                MAX(m.updated_at, COALESCE(m.accessed_at, '')),
                (SELECT COUNT(*) FROM relations r
                  WHERE (r.from_id = m.id OR r.to_id = m.id) AND r.valid_to IS NULL)
         FROM mem_items m WHERE {STALE_PREDICATE}
         ORDER BY julianday(m.created_at) ASC, m.id ASC LIMIT ?2"
    );
    let cutoff_text = cutoff.to_rfc3339_opts(SecondsFormat::Secs, true);
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("prepare stale: {e}"))?;
    let rows = stmt
        .query_map(params![julian_day(cutoff), limit as i64], |r| {
            let created_at: String = r.get(3)?;
            let last_touch: String = r.get(4)?;
            let active_neighbours: i64 = r.get(5)?;
            let reason = format!(
                "stale: created {created_at}, last direct touch {last_touch}, \
                 no relation change and no touched neighbour (of {active_neighbours}) since {cutoff_text}"
            );
            Ok(StaleSelection {
                id: r.get(0)?,
                kind: r.get(1)?,
                importance: r.get(2)?,
                created_at,
                last_touch,
                active_neighbours,
                reason,
            })
        })
        .map_err(|e| format!("query stale: {e}"))?;
    rows.collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("row stale: {e}"))
}

fn archived_today(conn: &Connection, now: DateTime<Utc>) -> Result<i64, String> {
    conn.query_row(
        "SELECT COUNT(*) FROM mem_item_history
         WHERE changed_by = ?1 AND julianday(changed_at) >= ?2",
        params![STALE_ACTOR, julian_day(utc_day_start(now))],
        |r| r.get(0),
    )
    .map_err(|e| format!("stale nightly count: {e}"))
}

/// Selects by [`STALE_PREDICATE`] up to the night's remaining cap. A dry run
/// lists ids and reasons and writes nothing; an apply needs the config gate.
pub(crate) fn run_stale_archive(
    conn: &Connection,
    cfg: &DreamHygieneConfig,
    now: DateTime<Utc>,
    mode: StaleMode,
    dry_run_limit: Option<usize>,
) -> Result<serde_json::Value, String> {
    if mode == StaleMode::Apply && !cfg.stale_archive {
        return Ok(serde_json::json!({
            "enabled": false,
            "dry_run": false,
            "skipped_reason": "disabled",
            "archived": [],
        }));
    }
    let cutoff = stale_cutoff(now);
    let cap = i64::from(cfg.stale_archive_cap);
    let done_today = archived_today(conn, now)?;
    let remaining = (cap - done_today).max(0) as usize;
    let matching = count_stale(conn, julian_day(cutoff))?;
    let mut report = serde_json::json!({
        "enabled": cfg.stale_archive,
        "dry_run": mode == StaleMode::DryRun,
        "cutoff": cutoff.to_rfc3339_opts(SecondsFormat::Secs, true),
        "cap": cap,
        "archived_today": done_today,
        "matching": matching,
    });

    if mode == StaleMode::DryRun {
        let limit = dry_run_limit.unwrap_or(remaining).min(MAX_DRY_RUN_LIMIT);
        report["selected"] =
            serde_json::to_value(select_stale(conn, cutoff, limit)?).map_err(|e| e.to_string())?;
        return Ok(report);
    }
    if remaining == 0 {
        report["skipped_reason"] = "nightly_cap".into();
        report["archived"] = serde_json::json!([]);
        return Ok(report);
    }

    let selected = select_stale(conn, cutoff, remaining)?;
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;
    let applied = selected.iter().try_for_each(|item| {
        archive_item(
            conn,
            &item.id,
            &item.kind,
            STALE_ACTOR,
            "archive",
            Some(&item.reason),
        )
    });
    if let Err(e) = applied {
        conn.execute_batch("ROLLBACK").ok();
        return Err(e);
    }
    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    report["archived"] = selected
        .iter()
        .map(|item| serde_json::json!({ "id": item.id, "reason": item.reason }))
        .collect();
    Ok(report)
}

#[derive(Deserialize, Default)]
pub struct StaleParams {
    #[serde(default)]
    pub dry_run: Option<bool>,
    #[serde(default)]
    pub limit: Option<usize>,
}

/// Shared by the HTTP and socket transports; read config before taking the mem lock.
pub(crate) fn stale_from_params(
    conn: &Connection,
    p: &StaleParams,
    hygiene: &DreamHygieneConfig,
) -> Result<serde_json::Value, String> {
    let mode = if p.dry_run.unwrap_or(false) {
        StaleMode::DryRun
    } else {
        StaleMode::Apply
    };
    run_stale_archive(conn, hygiene, Utc::now(), mode, p.limit)
}

pub async fn mem_calibration_stale(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<StaleParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let svc = state.svc.clone();
    let hygiene = crate::config::memory_config().dream_hygiene;
    tokio::task::spawn_blocking(move || {
        svc.mem
            .with_conn(|conn| stale_from_params(conn, &p, &hygiene))
    })
    .await
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, format!("join: {e}")))?
    .map(Json)
    .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))
}

#[cfg(test)]
mod tests;
