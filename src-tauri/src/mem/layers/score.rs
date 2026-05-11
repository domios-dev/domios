//! Pure score formula + filter SQL for L1 selection.
//!
//! score = importance
//!       + log10(1 + evidence_count) * 0.5
//!       + (workspace_match ? 0.5 : 0.0)
//!       + (recent_30d      ? 0.3 : 0.0)
//!
//! The score is computed in Rust (not SQL) because rusqlite's bundled SQLite
//! does not enable `SQLITE_ENABLE_MATH_FUNCTIONS` — `log10()` is missing.
//! Pulling raw fields and ranking in Rust keeps one formula source of truth.

pub fn score_for(importance: f64, evidence_count: i64, ws_match: bool, recent_30d: bool) -> f64 {
    importance
        + ((evidence_count as f64).max(0.0) + 1.0).log10() * 0.5
        + if ws_match { 0.5 } else { 0.0 }
        + if recent_30d { 0.3 } else { 0.0 }
}

/// Pass 1 — pinned items in scope.
/// Caller binds: ?1 = current_workspace_scope (e.g. "workspace:W1") or NULL.
pub const PASS1_SQL: &str = r#"
SELECT id FROM mem_items
WHERE is_pinned_wakeup = 1
  AND kind != 'archived'
  AND id != 'preference_identity'
  AND (scope = 'global' OR (?1 IS NOT NULL AND scope = ?1))
ORDER BY importance DESC, evidence_count DESC, updated_at DESC
LIMIT 50;
"#;

/// Pass 2 — unpinned candidates with raw fields for in-Rust scoring.
/// Caller binds: ?1 = current_workspace_scope or NULL.
/// Returns (id, importance, evidence_count, ws_match, recent_30d).
pub const PASS2_SQL: &str = r#"
SELECT
    id,
    importance,
    evidence_count,
    CASE WHEN ?1 IS NOT NULL AND scope = ?1 THEN 1 ELSE 0 END AS ws_match,
    CASE WHEN updated_at > datetime('now', '-30 days') THEN 1 ELSE 0 END AS recent_30d
FROM mem_items
WHERE is_pinned_wakeup = 0
  AND id != 'preference_identity'
  AND kind IN ('rule','preference','peer','fact','summary','skill','learning')
  AND (scope = 'global' OR (?1 IS NOT NULL AND scope = ?1))
  AND tags_json NOT LIKE '%auto-extracted%';
"#;
