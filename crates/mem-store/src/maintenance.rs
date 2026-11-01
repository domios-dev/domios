#[derive(Debug, Clone, serde::Serialize)]
pub struct WorkerDreamReport {
    pub considered: u64,
    pub promoted: u64,
    pub dropped: u64,
    pub phase: String,
    pub lessons_promoted: u64,
    pub lessons_already_high_confidence: u64,
}

pub fn worker_dreaming_light(
    conn: &rusqlite::Connection,
    machine: &str,
) -> Result<WorkerDreamReport, String> {
    let now_ts = chrono::Utc::now();
    let one_day_ago = (now_ts - chrono::Duration::days(1)).to_rfc3339();

    let candidates = light_candidates(conn, &one_day_ago)?;
    let considered = candidates.len() as u64;
    let mut promoted = 0;
    for candidate in &candidates {
        if promote_candidate(conn, candidate, now_ts, &PromotionAudit::system())? {
            promoted += 1;
        }
    }

    let promo = crate::lesson::promote_high_confidence_lessons(
        conn,
        crate::lesson::DEFAULT_PROMOTION_THRESHOLD,
        machine,
    )
    .map_err(|e| format!("lesson promotion: {}", e))?;

    Ok(WorkerDreamReport {
        considered,
        promoted,
        dropped: 0,
        phase: "light".into(),
        lessons_promoted: promo.promoted,
        lessons_already_high_confidence: promo.already_high_confidence,
    })
}

pub fn worker_dreaming_deep(conn: &rusqlite::Connection) -> Result<WorkerDreamReport, String> {
    let mut total_expired = 0u64;
    for (kind, days) in DEEP_TIERS {
        let n = expire_tier(conn, kind, *days)?;
        total_expired += n;
    }
    Ok(WorkerDreamReport {
        considered: total_expired,
        promoted: 0,
        dropped: total_expired,
        phase: "deep".into(),
        lessons_promoted: 0,
        lessons_already_high_confidence: 0,
    })
}

const NORMALIZE_PROTECTED_TYPES: &[&str] = &["kind", "area", "agent", "workspace"];
const NORMALIZE_ORPHAN_AGE_DAYS: i64 = 14;
const NORMALIZE_RATIO_HARD: f64 = 0.85;
const NORMALIZE_RATIO_FLAG: f64 = 0.70;
const NORMALIZE_SYNONYMS: &[(&str, &str)] = &[
    ("references", "mentions"),
    ("refers_to", "mentions"),
    ("contributes_to", "works_on"),
    ("at_location", "located_at"),
];

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

pub fn worker_dreaming_normalize(conn: &rusqlite::Connection) -> Result<serde_json::Value, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let cutoff =
        (chrono::Utc::now() - chrono::Duration::days(NORMALIZE_ORPHAN_AGE_DAYS)).to_rfc3339();

    let relations_canonicalized =
        crate::graph::canonicalize_relation_types(conn, NORMALIZE_SYNONYMS)
            .map_err(|e| format!("canonicalize: {}", e))?;

    let (merged, flagged) = run_merge_pass(conn).map_err(|e| format!("merge pass: {}", e))?;

    let orphans_deleted =
        crate::graph::cleanup_orphan_entities(conn, NORMALIZE_PROTECTED_TYPES, &cutoff)
            .map_err(|e| format!("orphan cleanup: {}", e))?;

    let broken_relations_invalidated = crate::graph::invalidate_broken_relations(conn, &now)
        .map_err(|e| format!("broken invalidate: {}", e))?;

    // Symmetric pass revives only edges with both endpoints still present.
    let revive_report = crate::graph::revive_broken(
        conn,
        crate::graph::ReviveBrokenOptions {
            since: None,
            dry_run: false,
        },
    )
    .map_err(|e| format!("broken revive: {}", e))?;

    Ok(serde_json::json!({
        "phase": "normalize",
        "merged": merged,
        "flagged_entity_pairs": flagged,
        "relations_canonicalized": relations_canonicalized,
        "orphans_deleted": orphans_deleted,
        "broken_relations_invalidated": broken_relations_invalidated,
        "broken_relations_revived": revive_report.revived,
    }))
}

fn run_merge_pass(conn: &rusqlite::Connection) -> Result<(u64, Vec<serde_json::Value>), String> {
    let entities = crate::graph::list_entities_with_degree(conn).map_err(|e| e.to_string())?;

    use std::collections::HashMap;
    type MergeBuckets = HashMap<(String, String), Vec<(String, String, u32)>>;
    let mut buckets: MergeBuckets = HashMap::new();
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

    let mut merged: u64 = 0;
    let mut flagged: Vec<serde_json::Value> = Vec::new();
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
                    if crate::graph::add_alias_with_rewire(conn, &alias, &canonical).is_ok() {
                        merged += 1;
                    }
                } else {
                    flagged.push(serde_json::json!({
                        "a": a_id, "b": b_id, "ratio": ratio,
                    }));
                }
            }
        }
    }
    Ok((merged, flagged))
}

#[cfg(test)]
mod tests;

pub type LightCandidate = (String, i64, f64, String, Option<String>);
pub fn light_candidates(
    conn: &rusqlite::Connection,
    before: &str,
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
    let rows = stmt
        .query_map(rusqlite::params![before], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, f64>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, Option<String>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(|r| r.ok()).collect())
}
pub fn promote_candidate(
    conn: &rusqlite::Connection,
    candidate: &LightCandidate,
    now_ts: chrono::DateTime<chrono::Utc>,
    audit: &PromotionAudit<'_>,
) -> Result<bool, String> {
    let (id, access_count, importance, created_at, accessed_at) = candidate;
    let frequency = (*access_count as f64 / 10.0).min(1.0);
    let recency = accessed_at
        .as_ref()
        .and_then(|a| chrono::DateTime::parse_from_rfc3339(a).ok())
        .map(|a| {
            let days = (now_ts - a.with_timezone(&chrono::Utc)).num_hours().max(0) as f64 / 24.0;
            (-days * 0.693 / 14.0).exp()
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

    let now = chrono::Utc::now().to_rfc3339();
    let n = conn.execute(
            "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2 AND author_kind != 'neighbor'",
            rusqlite::params![&now, id],
        ).map_err(|e| e.to_string())?;
    if n > 0 {
        // history_log mirror — best-effort, don't fail the run on history errors.
        let new_imp: f64 = importance.max(4.0);
        let _ = conn.execute(
                r#"INSERT INTO mem_item_history
                   (item_id, changed_at, changed_by, change_type,
                    old_importance, new_importance, old_kind, new_kind, author_kind, author_label,author_uid,author_friendship)
                   VALUES (?1, ?2, ?5, 'promote', ?3, ?4, 'learning', 'fact', ?6, ?7, ?8, ?9)"#,
                rusqlite::params![id, &now, importance, new_imp,audit.changed_by,audit.kind,audit.label,audit.uid,audit.friendship],
            );
        return Ok(true);
    }
    Ok(false)
}

pub const DEEP_TIERS: &[(&str, i64)] = &[
    ("conversation", 30),
    ("daily", 30),
    ("note", 60),
    ("script", 60),
    ("learning", 90),
];

pub fn expire_tier(conn: &rusqlite::Connection, kind: &str, days: i64) -> Result<u64, String> {
    let now = chrono::Utc::now().to_rfc3339();
    let cutoff = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
    let n = conn
        .execute(
            "UPDATE mem_items SET importance = 0.0, updated_at = ?1 \
             WHERE kind = ?2 AND access_count = 0 AND created_at < ?3 \
             AND importance > 0.0 AND importance < 4.0",
            rusqlite::params![&now, kind, &cutoff],
        )
        .map_err(|e| e.to_string())? as u64;
    Ok(n)
}

pub struct PromotionAudit<'a> {
    pub changed_by: &'a str,
    pub kind: &'a str,
    pub label: Option<&'a str>,
    pub uid: Option<&'a str>,
    pub friendship: Option<&'a str>,
}
impl PromotionAudit<'static> {
    pub fn system() -> Self {
        Self {
            changed_by: "dreaming:rem",
            kind: "system",
            label: Some("dreaming:rem"),
            uid: None,
            friendship: None,
        }
    }
}
