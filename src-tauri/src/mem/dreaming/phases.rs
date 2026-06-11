//! Test-only mirrors of the Light / Deep / Normalize SQL phases. Production
//! runs the same SQL inline in the `cli` dreaming workers; these fixtures keep
//! the backend and CLI logic verifiable under one set of unit tests.

use rusqlite::{params, Connection};

use super::DreamReport;
use crate::mem::MemManager;

/// Test-only fixture; production runs the SQL inline in `cli::worker_dreaming_light`.
/// Pull the CLI back under codeterm_lib later? Reintroduce a sync version and call it.
pub async fn light_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;

    const BATCH: usize = 8;

    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates = {
        let conn = mem.conn().lock();
        light_query_candidates(&conn, &one_day_ago)?
    };

    let mut considered = 0u64;
    let mut promoted = 0u64;

    for (batch_idx, chunk) in candidates.chunks(BATCH).enumerate() {
        for cand in chunk {
            considered += 1;
            let promote = {
                let conn = mem.conn().lock();
                light_promote_one(&conn, cand, now_ts)?
            };
            if promote {
                promoted += 1;
            }
        }

        if batch_idx + 1 < candidates.chunks(BATCH).len() {
            tokio::task::yield_now().await;
        }
    }

    Ok(DreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
    })
}

type LightCandidate = (String, i64, f64, String, Option<String>);

// Mirrored in `cli/src/main.rs::worker_dreaming_light`; keep SQL in sync.
fn light_query_candidates(
    conn: &Connection,
    one_day_ago: &str,
) -> Result<Vec<LightCandidate>, String> {
    let mut stmt = conn
        .prepare(
            r#"SELECT id, access_count, importance, created_at, accessed_at
               FROM mem_items
               WHERE kind = 'learning'
                 AND access_count >= 3
                 AND created_at < ?1"#,
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<_> = stmt
        .query_map(params![one_day_ago], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    Ok(rows)
}

// Mirrored in `cli/src/main.rs::worker_dreaming_light`; keep scoring + UPDATE + history INSERT in sync.
fn light_promote_one(
    conn: &Connection,
    cand: &LightCandidate,
    now_ts: chrono::DateTime<chrono::Utc>,
) -> Result<bool, String> {
    let (id, access_count, importance, created_at, accessed_at) = cand;

    let frequency = (*access_count as f64 / 10.0).min(1.0);

    let recency = accessed_at
        .as_ref()
        .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
        .map(|a| {
            let days = (now_ts - a.with_timezone(&chrono::Utc)).num_hours().max(0) as f64 / 24.0;
            (-days * 0.693 / 14.0).exp() // ln(2)/14 ≈ 0.0495
        })
        .unwrap_or(0.0);

    let imp_score = (*importance / 5.0).min(1.0);

    let age = chrono::DateTime::parse_from_rfc3339(created_at)
        .ok()
        .map(|c| {
            let days = (now_ts - c.with_timezone(&chrono::Utc)).num_hours().max(0) as f64 / 24.0;
            (days / 7.0).min(1.0)
        })
        .unwrap_or(0.5);

    let score = frequency * 0.30 + recency * 0.25 + imp_score * 0.25 + age * 0.20;

    if score < 0.5 {
        return Ok(false);
    }

    let new_imp = importance.max(4.0);
    let now = chrono::Utc::now().to_rfc3339();
    let n = crate::mem::store::with_changed_by("dreaming:rem", || {
        let n = conn
            .execute(
                "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
                params![&now, id],
            )
            .map_err(|e| e.to_string())?;
        if n > 0 {
            crate::mem::store::history_log(
                conn,
                id,
                "promote",
                &crate::mem::store::HistorySnapshot {
                    importance: Some(*importance),
                    kind: Some("learning".into()),
                    ..Default::default()
                },
                &crate::mem::store::HistorySnapshot {
                    importance: Some(new_imp),
                    kind: Some("fact".into()),
                    ..Default::default()
                },
                None,
            );
        }
        Ok::<usize, String>(n)
    })?;
    Ok(n > 0)
}

/// Test-only fixture; production runs the SQL inline in `cli::worker_dreaming_deep`.
/// Pull the CLI back under codeterm_lib later? Reintroduce a sync version and call it.
pub async fn deep_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;

    let tiers = deep_tiers();
    let mut total_expired = 0u64;

    for (idx, (kind, days)) in tiers.iter().enumerate() {
        let expired = {
            let conn = mem.conn().lock();
            deep_expire_tier(&conn, kind, *days)?
        };
        total_expired += expired;

        if idx + 1 < tiers.len() {
            tokio::task::yield_now().await;
        }
    }

    Ok(DreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
    })
}

// Mirrored in `cli/src/main.rs::worker_dreaming_deep`; keep tier table in sync.
fn deep_tiers() -> &'static [(&'static str, i64)] {
    &[
        ("conversation", 30),
        ("daily", 30),
        ("note", 60),
        ("script", 60),
        ("learning", 90),
    ]
}

// Mirrored in `cli/src/main.rs::worker_dreaming_deep`; keep UPDATE in sync.
fn deep_expire_tier(conn: &Connection, kind: &str, days: i64) -> Result<u64, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let n = conn
        .execute(
            "UPDATE mem_items SET importance = 0.0, updated_at = ?1 \
             WHERE kind = ?2 AND access_count = 0 AND created_at < ?3 \
             AND importance > 0.0 AND importance < 4.0",
            params![&now, kind, &cutoff],
        )
        .map_err(|e| e.to_string())? as u64;
    Ok(n)
}

// Graph normalize test-only mirror so CLI worker and backend share graph
// normalization logic and same `codeterm-mem-store` behavior.

const NORMALIZE_PROTECTED_TYPES: &[&str] = &["kind", "area", "agent", "workspace"];
const NORMALIZE_ORPHAN_AGE_DAYS: i64 = 14;
// Typo thresholds tuned so near-matches like "tima" vs "timofey" (~0.4) stay ignored.
const NORMALIZE_RATIO_HARD: f64 = 0.85;
const NORMALIZE_RATIO_FLAG: f64 = 0.70;
const NORMALIZE_SYNONYMS: &[(&str, &str)] = &[
    ("references", "mentions"),
    ("refers_to", "mentions"),
    ("contributes_to", "works_on"),
    ("at_location", "located_at"),
];

#[derive(Default, Debug)]
pub struct NormalizeReport {
    pub merged: u64,
    pub flagged_pairs: Vec<(String, String, f64)>,
    pub relations_canonicalized: u64,
    pub orphans_deleted: u64,
    pub broken_relations_invalidated: u64,
}

fn lev(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    if n == 0 {
        return m;
    }
    if m == 0 {
        return n;
    }
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut curr = vec![0usize; m + 1];
    for i in 1..=n {
        curr[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            curr[j] = (prev[j] + 1).min(curr[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[m]
}

fn lev_ratio(a: &str, b: &str) -> f64 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - (lev(a, b) as f64 / max as f64)
}

fn split_prefix(id: &str) -> Option<(&str, &str)> {
    id.find('_').map(|i| (&id[..i], &id[i + 1..]))
}

/// Test-only fixture; production runs the SQL inline in `cli::worker_dreaming_normalize`.
pub async fn normalize_phase(mem: &MemManager) -> Result<DreamReport, String> {
    let _permit = mem.acquire_heavy_permit().await;
    let conn = mem.conn();
    let conn = conn.lock();
    let r = normalize_run(&conn)?;
    Ok(DreamReport {
        considered: r.flagged_pairs.len() as u64,
        promoted: r.merged,
        dropped: r.orphans_deleted + r.broken_relations_invalidated,
        phase: "normalize".into(),
    })
}

fn normalize_run(conn: &Connection) -> Result<NormalizeReport, String> {
    let mut report = NormalizeReport::default();
    let now = chrono::Utc::now().to_rfc3339();

    report.relations_canonicalized =
        codeterm_mem_store::graph::canonicalize_relation_types(conn, NORMALIZE_SYNONYMS)
            .map_err(|e| format!("canonicalize: {}", e))?;

    let entities = codeterm_mem_store::graph::list_entities_with_degree(conn)
        .map_err(|e| format!("list entities: {}", e))?;

    use std::collections::HashMap;
    let mut buckets: HashMap<(String, String), Vec<(String, String, u32)>> = HashMap::new();
    for e in &entities {
        if let Some((prefix, suffix)) = split_prefix(&e.id) {
            if suffix.chars().count() >= 2 {
                buckets
                    .entry((e.entity_type.clone(), prefix.to_string()))
                    .or_default()
                    .push((e.id.clone(), suffix.to_string(), e.degree));
            }
        }
    }

    for entries in buckets.values() {
        for i in 0..entries.len() {
            for j in (i + 1)..entries.len() {
                let (a_id, a_suf, a_deg) = &entries[i];
                let (b_id, b_suf, b_deg) = &entries[j];
                let ratio = lev_ratio(a_suf, b_suf);
                if ratio < NORMALIZE_RATIO_FLAG {
                    continue;
                }
                if ratio >= NORMALIZE_RATIO_HARD && a_deg != b_deg {
                    let (alias, canonical) = if a_deg < b_deg {
                        (a_id.clone(), b_id.clone())
                    } else {
                        (b_id.clone(), a_id.clone())
                    };
                    if codeterm_mem_store::graph::add_alias_with_rewire(conn, &alias, &canonical)
                        .is_ok()
                    {
                        report.merged += 1;
                    }
                } else {
                    report
                        .flagged_pairs
                        .push((a_id.clone(), b_id.clone(), ratio));
                }
            }
        }
    }

    let cutoff =
        (chrono::Utc::now() - chrono::Duration::days(NORMALIZE_ORPHAN_AGE_DAYS)).to_rfc3339();
    report.orphans_deleted = codeterm_mem_store::graph::cleanup_orphan_entities(
        conn,
        NORMALIZE_PROTECTED_TYPES,
        &cutoff,
    )
    .map_err(|e| format!("orphan cleanup: {}", e))?;

    report.broken_relations_invalidated =
        codeterm_mem_store::graph::invalidate_broken_relations(conn, &now)
            .map_err(|e| format!("broken invalidate: {}", e))?;

    Ok(report)
}
