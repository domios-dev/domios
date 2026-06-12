//! Lesson promotion scaffold (Track 9 of v1.1.23, Issue #59).
//!
//! Light-phase scan: kind='lesson' rows with enough corroborating evidence
//! and zero false-fires get the `high-confidence` tag plus an audit row in
//! `lesson_audit`. Cross-mesh replication is deferred to v1.1.24.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::MemStoreResult;

pub const DEFAULT_PROMOTION_THRESHOLD: u32 = 10;
const HIGH_CONFIDENCE_TAG: &str = "high-confidence";
const PROMOTION_REASON: &str = "auto: evidence_count threshold met";

#[derive(Debug, Clone, Serialize)]
pub struct PromotionReport {
    pub promoted: u64,
    pub already_high_confidence: u64,
}

/// Light-phase scan. For each `kind=lesson` row where evidence_count >= threshold
/// (per-item override wins over `default_threshold`) AND false_fire_count = 0:
///   - if not already tagged 'high-confidence': add tag, write audit row;
///   - if already tagged: count under `already_high_confidence`.
/// Idempotent: re-running yields 0 promoted.
pub fn promote_high_confidence_lessons(
    conn: &Connection,
    default_threshold: u32,
    machine_id: &str,
) -> MemStoreResult<PromotionReport> {
    let now = chrono::Utc::now().to_rfc3339();
    let tx = conn.unchecked_transaction()?;

    let candidates: Vec<(String, String)> = {
        let mut stmt = tx.prepare(
            "SELECT id, tags_json FROM mem_items \
             WHERE kind = 'lesson' \
               AND false_fire_count = 0 \
               AND evidence_count >= COALESCE(promotion_threshold, ?1)",
        )?;
        let rows = stmt.query_map(rusqlite::params![default_threshold as i64], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?;
        rows.filter_map(Result::ok).collect()
    };

    let mut promoted: u64 = 0;
    let mut already: u64 = 0;

    for (id, tags_json) in candidates {
        let mut tags = parse_tags(&tags_json);
        if tags.iter().any(|t| t == HIGH_CONFIDENCE_TAG) {
            already += 1;
            continue;
        }
        tags.push(HIGH_CONFIDENCE_TAG.to_string());
        let new_tags = serde_json::to_string(&tags)
            .unwrap_or_else(|_| format!("[\"{}\"]", HIGH_CONFIDENCE_TAG));
        tx.execute(
            "UPDATE mem_items SET tags_json = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![&new_tags, &now, &id],
        )?;
        tx.execute(
            "INSERT INTO lesson_audit (lesson_uid, machine_id, action, ts, reason) \
             VALUES (?1, ?2, 'promoted', ?3, ?4)",
            rusqlite::params![&id, machine_id, &now, PROMOTION_REASON],
        )?;
        promoted += 1;
    }

    tx.commit()?;
    Ok(PromotionReport {
        promoted,
        already_high_confidence: already,
    })
}

fn parse_tags(tags_json: &str) -> Vec<String> {
    serde_json::from_str::<Vec<String>>(tags_json).unwrap_or_default()
}
