//! Dreaming consolidation — promotes high-signal mem_items based on access
//! frequency and importance. Light + Deep phases run as a cron-spawned agent
//! by default; can also be invoked manually via `mem dream now`.
//!
//! In Phase 3.4 dreaming runs locally (no LLM call) — pure SQL signals over
//! the existing `accessed_at` and `importance` columns. The full vision in
//! mem-architecture.md (Light → Deep → REM with token budgets) is layered on
//! top of this baseline in later phases as needed.

use rusqlite::params;
#[cfg(test)]
use rusqlite::Connection;
use serde::{Deserialize, Serialize};

use super::MemManager;

pub mod recluster;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DreamReport {
    pub considered: u64,
    pub promoted: u64,
    pub dropped: u64,
    pub phase: String,
}

/// Test-only fixture; production runs the SQL inline in `cli::worker_dreaming_light`.
/// Pull the CLI back under codeterm_lib later? Reintroduce a sync version and call it.
#[cfg(test)]
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

#[cfg(test)]
type LightCandidate = (String, i64, f64, String, Option<String>);

// Mirrored in `cli/src/main.rs::worker_dreaming_light`; keep SQL in sync.
#[cfg(test)]
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
#[cfg(test)]
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
    let n = super::store::with_changed_by("dreaming:rem", || {
        let n = conn
            .execute(
                "UPDATE mem_items SET kind = 'fact', importance = MAX(importance, 4.0), updated_at = ?1 WHERE id = ?2",
                params![&now, id],
            )
            .map_err(|e| e.to_string())?;
        if n > 0 {
            super::store::history_log(
                conn,
                id,
                "promote",
                &super::store::HistorySnapshot {
                    importance: Some(*importance),
                    kind: Some("learning".into()),
                    ..Default::default()
                },
                &super::store::HistorySnapshot {
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
#[cfg(test)]
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
#[cfg(test)]
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
#[cfg(test)]
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

// Graph normalize phase. Test-only mirror — graph mutations route through
// codeterm-mem-store; the merge-pass orchestration (Levenshtein loop) is
// kept here so the CLI worker and this module behave identically against the
// same DB. Constants live in the CLI worker (caller-owned policy).

#[cfg(test)]
const NORMALIZE_PROTECTED_TYPES: &[&str] = &["kind", "area", "agent", "workspace"];
#[cfg(test)]
const NORMALIZE_ORPHAN_AGE_DAYS: i64 = 14;
// Thresholds calibrated for typo-class similarity. Nickname↔fullname pairs
// (e.g. tima↔timofey) yield ratios ~0.4 and are intentionally ignored.
#[cfg(test)]
const NORMALIZE_RATIO_HARD: f64 = 0.85;
#[cfg(test)]
const NORMALIZE_RATIO_FLAG: f64 = 0.70;
#[cfg(test)]
const NORMALIZE_SYNONYMS: &[(&str, &str)] = &[
    ("references", "mentions"),
    ("refers_to", "mentions"),
    ("contributes_to", "works_on"),
    ("at_location", "located_at"),
];

#[cfg(test)]
#[derive(Default, Debug)]
pub struct NormalizeReport {
    pub merged: u64,
    pub flagged_pairs: Vec<(String, String, f64)>,
    pub relations_canonicalized: u64,
    pub orphans_deleted: u64,
    pub broken_relations_invalidated: u64,
}

#[cfg(test)]
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

#[cfg(test)]
fn lev_ratio(a: &str, b: &str) -> f64 {
    let max = a.chars().count().max(b.chars().count());
    if max == 0 {
        return 1.0;
    }
    1.0 - (lev(a, b) as f64 / max as f64)
}

#[cfg(test)]
fn split_prefix(id: &str) -> Option<(&str, &str)> {
    id.find('_').map(|i| (&id[..i], &id[i + 1..]))
}

/// Test-only fixture; production runs the SQL inline in `cli::worker_dreaming_normalize`.
#[cfg(test)]
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

#[cfg(test)]
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

/// Prepare data for the REM agent — returns a text summary of candidates.
/// Called by `codeterm mem dream --report` so the agent gets concrete data.
pub fn prepare_rem_data(mem: &MemManager) -> Result<String, String> {
    let conn = mem.conn();
    let conn = conn.lock();
    let mut lines = Vec::new();

    // 1. Unclassified items (no area)
    let mut stmt = conn
        .prepare("SELECT id, kind, title FROM mem_items WHERE (area IS NULL OR area = '') LIMIT 20")
        .map_err(|e| e.to_string())?;
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
    let mut stmt = conn
        .prepare(
            r#"SELECT a.id, b.id, a.kind, a.title, b.title, a.importance, b.importance
           FROM mem_items a
           JOIN mem_items b ON a.kind = b.kind
             AND a.id < b.id
             AND substr(a.title, 1, 12) = substr(b.title, 1, 12)
             AND a.title != b.title
           LIMIT 20"#,
        )
        .map_err(|e| e.to_string())?;
    let dupes: Vec<(String, String, String, String, String, f64, f64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !dupes.is_empty() {
        lines.push(format!("\nPOTENTIAL DUPLICATES ({}):", dupes.len()));
        for (a_id, b_id, kind, a_title, b_title, a_imp, b_imp) in &dupes {
            lines.push(format!(
                "  [{}] \"{}\" (imp={}) vs \"{}\" (imp={})",
                kind, a_title, a_imp, b_title, b_imp
            ));
            lines.push(format!("    ids: {} vs {}", a_id, b_id));
        }
    }

    // 3. Recent high-access items (last 7 days, for pattern finding)
    let week_ago = (chrono::Utc::now() - chrono::Duration::days(7)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            r#"SELECT id, kind, title, area, access_count, recurrence_count
           FROM mem_items
           WHERE updated_at > ?1 AND access_count >= 2
           ORDER BY access_count DESC
           LIMIT 15"#,
        )
        .map_err(|e| e.to_string())?;
    let hot: Vec<(String, String, String, Option<String>, i64, i64)> = stmt
        .query_map(params![&week_ago], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !hot.is_empty() {
        lines.push(format!("\nHOT ITEMS last 7d ({}):", hot.len()));
        for (id, kind, title, area, ac, rc) in &hot {
            let recur = if *rc > 0 {
                format!(" recur={}", rc)
            } else {
                String::new()
            };
            lines.push(format!(
                "  {} [{}] area={} access={}{} {}",
                id,
                kind,
                area.as_deref().unwrap_or("?"),
                ac,
                recur,
                title
            ));
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

    // 5. Recurring items (recurrence_count > 0 — system flagged as saved multiple times)
    let mut stmt = conn
        .prepare(
            r#"SELECT id, kind, title, area, recurrence_count, importance
           FROM mem_items
           WHERE recurrence_count > 0
           ORDER BY recurrence_count DESC
           LIMIT 15"#,
        )
        .map_err(|e| e.to_string())?;
    let recurring: Vec<(String, String, String, Option<String>, i64, f64)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !recurring.is_empty() {
        lines.push(format!("\nRECURRING ITEMS ({}):", recurring.len()));
        for (id, kind, title, area, rc, imp) in &recurring {
            lines.push(format!(
                "  {} [{}] area={} recur={} imp={} {}",
                id,
                kind,
                area.as_deref().unwrap_or("?"),
                rc,
                imp,
                title
            ));
        }
    }

    // 6. Stale learnings (>30 days, low access — candidates for promotion or archival)
    let month_ago = (chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339();
    let mut stmt = conn
        .prepare(
            r#"SELECT id, title, access_count, importance, created_at
           FROM mem_items
           WHERE kind = 'learning' AND created_at < ?1
           ORDER BY access_count DESC, importance DESC
           LIMIT 20"#,
        )
        .map_err(|e| e.to_string())?;
    let stale_learnings: Vec<(String, String, i64, f64, String)> = stmt
        .query_map(params![&month_ago], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    if !stale_learnings.is_empty() {
        lines.push(format!(
            "\nLEARNINGS >30d ({}) — promote or archive?:",
            stale_learnings.len()
        ));
        for (id, title, ac, imp, created) in &stale_learnings {
            lines.push(format!(
                "  {} access={} imp={} created={} {}",
                id,
                ac,
                imp,
                &created[..10],
                title
            ));
        }
    }

    // 7. Graph stats
    let entity_count: i64 = conn
        .query_row("SELECT COUNT(*) FROM entities", [], |r| r.get(0))
        .unwrap_or(0);
    let relation_count: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM relations WHERE valid_to IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let orphan_entities: i64 = conn.query_row(
        "SELECT COUNT(*) FROM entities WHERE id NOT IN (SELECT from_id FROM relations) AND id NOT IN (SELECT to_id FROM relations)",
        [], |r| r.get(0),
    ).unwrap_or(0);
    lines.push(format!(
        "\nGRAPH: {} entities, {} active relations, {} orphan entities",
        entity_count, relation_count, orphan_entities
    ));

    // 8. Kind distribution
    let mut stmt = conn
        .prepare("SELECT kind, COUNT(*) FROM mem_items GROUP BY kind ORDER BY COUNT(*) DESC")
        .map_err(|e| e.to_string())?;
    let kinds: Vec<(String, i64)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();
    lines.push("\nKIND DISTRIBUTION:".into());
    for (kind, count) in &kinds {
        lines.push(format!("  {}: {}", kind, count));
    }

    if lines.is_empty() {
        Ok("Nothing to optimize.".into())
    } else {
        Ok(lines.join("\n"))
    }
}

const DREAMING_TELEGRAM_STEP_ENABLED: &str = r#"Then push the INSIGHT (only the insight prose, not the log block) to
telegram so the vault talks back. OK if it errors — the diary in mem
is the durable artifact:

    codeterm mem search "telegram chat id" --limit 1
    # extract the chat id, then:
    codeterm bridge send --to <chat_id> --service telegram \
      "🧠 Dreamer brief\n\n<just the insight, no <insight> tags, no log>"
    # If no fact found or bridge errors, skip — telegram is nice-to-have."#;

const DREAMING_TELEGRAM_STEP_DISABLED: &str = "Telegram notification is DISABLED \
    (memory.dreaming_telegram_notify=false). Skip any outbound notification — \
    do not search for chat ids, do not invoke the bridge command. The diary \
    in mem is the artifact; the user has opted out of the nightly Telegram ping.";

/// Spawn a headless dreaming worker for REM phase (dedup, classify, insights, diary).
/// Called after the supervisor runs the SQL phases.
pub fn spawn_rem_worker(svc: &std::sync::Arc<crate::service::ServiceContext>) {
    let mem_cfg = crate::config::memory_config();
    let agent = if mem_cfg.dreaming_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.dreaming_agent.clone()
    };
    // Dreaming REM worker uses `dreaming_model` when set, otherwise falls
    // back to the shared `worker_model` default ("haiku").
    let raw_model = if mem_cfg.dreaming_model.is_empty() {
        mem_cfg.worker_model.clone()
    } else {
        mem_cfg.dreaming_model.clone()
    };
    let model = crate::agent::models::canonicalize_short(&raw_model).to_string();

    // Substitute the conditional telegram-notify step. When the config flag
    // is enabled (default), the dreamer pushes its one-line insight to the
    // Telegram bridge after writing the diary. When disabled, the dreamer
    // is instructed to skip the step entirely — the diary in mem stays the
    // durable record either way.
    let telegram_step = if mem_cfg.dreaming_telegram_notify {
        DREAMING_TELEGRAM_STEP_ENABLED
    } else {
        DREAMING_TELEGRAM_STEP_DISABLED
    };
    let prompt = crate::prompts::DREAMING_REM.replace("{telegram_notify_step}", telegram_step);

    match super::workers::spawn_headless(
        svc,
        format!("{}rem", super::workers::DREAMING_INTENT_PREFIX),
        prompt,
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
/// - Schedule: "0 6 * * *" (6 AM)
/// - Jitter: ±30 minutes (1_800_000 ms) — fires anywhere 5:30–6:30 AM
/// - Task: runs `codeterm mem dream` in a fresh agent
/// - Workspace: "general" (the GA workspace)
///
/// Returns true if a new cron was created, false if one already existed.
pub fn ensure_dreaming_cron(
    cron_store: &mut crate::cron::store::CronStore,
) -> Result<bool, String> {
    let mem_cfg = crate::config::memory_config();

    if let Some(job) = cron_store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
    {
        let desired_agent = if mem_cfg.dreaming_agent.is_empty() {
            crate::config::default_agent_provider()
        } else {
            mem_cfg.dreaming_agent.clone()
        };
        let agent_changed = job.agent != desired_agent;
        let legacy_schedule_match = matches!(job.schedule.as_str(), "0 3 * * *");
        let legacy_jitter_match = job.jitter_max_ms == Some(5_400_000);

        if agent_changed || legacy_schedule_match || legacy_jitter_match {
            let job_id = job.id.clone();
            if let Some(j) = cron_store.get_mut(&job_id) {
                if agent_changed {
                    j.agent = desired_agent;
                }
                if legacy_schedule_match {
                    j.schedule = "0 6 * * *".into();
                }
                if legacy_jitter_match {
                    j.jitter_max_ms = Some(1_800_000);
                }
                // Issue #96: recompute next_run after schedule/jitter migration.
                // Without this, the on-disk next_run still reflects the legacy
                // "0 3 * * *" + 90-min-jitter window, which is in the past
                // relative to the new "0 6" window. On daemon startup the
                // scheduler sees the stale value, fires a "recovery" run, and
                // record_run then anchors the next fire to today's 06:00
                // window — producing the double-fire pattern reported as #96.
                if j.enabled && !j.oneshot {
                    // Only overwrite next_run when compute returned a concrete
                    // Some(_). Ok(None) means no future occurrence exists for
                    // this schedule — clearing next_run silently would stop
                    // the job firing, which is exactly the failure mode #96
                    // was fixing. Log + keep the old value in that case.
                    match crate::cron::store::compute_next_run_jittered_in(
                        &j.schedule,
                        j.jitter_max_ms,
                        j.tz.as_deref(),
                    ) {
                        Ok(Some(nr)) => j.next_run = Some(nr),
                        Ok(None) => log::warn!(
                            "[dreaming] schedule {:?} has no upcoming occurrence; \
                             leaving next_run={:?} unchanged",
                            j.schedule,
                            j.next_run,
                        ),
                        Err(e) => log::warn!(
                            "[dreaming] recompute next_run after migration failed: {}; \
                             leaving next_run={:?} unchanged",
                            e,
                            j.next_run,
                        ),
                    }
                }
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

    // Prefer the specific `dreaming_model` override; otherwise persist the
    // shared `worker_model` on the cron so manual executions keep parity.
    let model = if !mem_cfg.dreaming_model.is_empty() {
        Some(mem_cfg.dreaming_model.clone())
    } else if !mem_cfg.worker_model.is_empty() {
        Some(mem_cfg.worker_model.clone())
    } else {
        None
    }
    .map(|s| crate::agent::models::canonicalize_short(&s).to_string());

    let _ = cron_store.create(crate::cron::store::CronCreateParams {
        name: DREAMING_CRON_NAME.into(),
        workspace_id: String::new(),
        schedule: "0 6 * * *".into(),
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
        jitter_max_ms: Some(1_800_000),
        tz: None,
        timeout_seconds: Some(mem_cfg.dreaming_timeout_seconds),
    })?;
    // Mark as protected after creation
    if let Some(job) = cron_store
        .list()
        .iter()
        .find(|j| j.name == DREAMING_CRON_NAME)
    {
        let job_id = job.id.clone();
        if let Some(j) = cron_store.get_mut(&job_id) {
            j.protected = true;
        }
        let _ = cron_store.save();
    }
    Ok(true)
}

pub const LIBRARIAN_CRON_NAME: &str = "mem-librarian-auto";

/// Removes the legacy LLM-based librarian cron if it still exists from older
/// installs. The librarian now runs as a CLI subprocess via the supervisor;
/// kept as a migration step.
pub fn remove_legacy_librarian_cron(cron_store: &mut crate::cron::store::CronStore) -> bool {
    let existing = cron_store
        .list()
        .iter()
        .find(|j| j.name == LIBRARIAN_CRON_NAME)
        .map(|j| j.id.clone());
    if let Some(id) = existing {
        if let Some(job) = cron_store.get_mut(&id) {
            job.protected = false;
        }
        let _ = cron_store.delete(&id);
        let _ = cron_store.save();
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mem::store::{MemKind, MemScope, NewMemItem};
    use chrono::Timelike;

    #[tokio::test]
    async fn light_phase_promotes_accessed_learnings() {
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
                source_path: None,
                pinned: false,
            })
            .unwrap();

        {
            let conn = mem.conn();
            let conn = conn.lock();
            let two_days_ago = (chrono::Utc::now() - chrono::Duration::days(2)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1, accessed_at = ?2, access_count = 3 WHERE id = ?3",
                params![&two_days_ago, &two_days_ago, &id],
            )
            .unwrap();
        }

        let report = light_phase(&mem).await.unwrap();
        assert_eq!(report.considered, 1);
        assert_eq!(report.promoted, 1);

        let after = mem.get(&id).unwrap().unwrap();
        assert_eq!(after.kind, "fact");
        assert!(after.importance >= 4.0);
    }

    #[tokio::test]
    async fn deep_phase_expires_old_unaccessed_conversations() {
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
                source_path: None,
                pinned: false,
            })
            .unwrap();

        // Backdate to 60 days ago, never accessed
        {
            let conn = mem.conn();
            let conn = conn.lock();
            let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1, access_count = 0 WHERE id = ?2",
                params![&sixty_days_ago, &id],
            )
            .unwrap();
        }

        let report = deep_phase(&mem).await.unwrap();
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

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(job.jitter_max_ms, Some(1_800_000), "jitter must be ±30 min");
        assert!(job.enabled);
        assert_eq!(job.schedule, "0 6 * * *");
    }

    #[test]
    fn legacy_jitter_90min_migrates_to_30min() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        ensure_dreaming_cron(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();
        if let Some(j) = store.get_mut(&id) {
            j.jitter_max_ms = Some(5_400_000);
        }
        let _ = store.save();

        ensure_dreaming_cron(&mut store).unwrap();

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(
            job.jitter_max_ms,
            Some(1_800_000),
            "legacy 90min must be auto-migrated to 30min"
        );
    }

    #[test]
    fn legacy_3am_schedule_migrates_to_6am() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        ensure_dreaming_cron(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();
        if let Some(j) = store.get_mut(&id) {
            j.schedule = "0 3 * * *".into();
        }
        let _ = store.save();

        ensure_dreaming_cron(&mut store).unwrap();

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(
            job.schedule, "0 6 * * *",
            "legacy 3am must be auto-migrated to 6am"
        );
    }

    #[test]
    fn dreaming_telegram_step_substitution() {
        // Enabled (default): the placeholder must expand to the real bridge-send
        // instructions so the dreamer pushes its 🧠 brief to Telegram.
        let enabled = crate::prompts::DREAMING_REM.replace(
            "{telegram_notify_step}",
            super::DREAMING_TELEGRAM_STEP_ENABLED,
        );
        assert!(
            enabled.contains("codeterm bridge send"),
            "enabled prompt must instruct bridge send"
        );
        assert!(
            enabled.contains("🧠 Dreamer brief"),
            "enabled prompt must keep the brief tag"
        );
        assert!(
            !enabled.contains("{telegram_notify_step}"),
            "placeholder must be substituted, not literal"
        );

        // Disabled: the placeholder expands to an explicit "skip" directive
        // and the bridge-send command must NOT appear in the final prompt.
        let disabled = crate::prompts::DREAMING_REM.replace(
            "{telegram_notify_step}",
            super::DREAMING_TELEGRAM_STEP_DISABLED,
        );
        assert!(
            !disabled.contains("codeterm bridge send"),
            "disabled prompt must NOT mention bridge send anywhere"
        );
        assert!(
            disabled.contains("DISABLED"),
            "disabled prompt must explicitly flag opt-out"
        );
        assert!(
            disabled.contains("dreaming_telegram_notify"),
            "disabled prompt must cite the config key so user knows what to flip"
        );
    }

    #[test]
    fn legacy_schedule_migration_recomputes_next_run() {
        // Issue #96: migrating "0 3 * * *" -> "0 6 * * *" must also reset
        // next_run, else the on-disk value points at the OLD schedule's
        // window and the scheduler fires a recovery run on startup.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        ensure_dreaming_cron(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();

        // Simulate legacy state: stale next_run from the old "0 3 * * *" cadence.
        let stale_legacy_next =
            crate::cron::store::compute_next_run_jittered_in("0 3 * * *", Some(5_400_000), None)
                .unwrap()
                .unwrap();
        if let Some(j) = store.get_mut(&id) {
            j.schedule = "0 3 * * *".into();
            j.jitter_max_ms = Some(5_400_000);
            j.next_run = Some(stale_legacy_next);
        }
        let _ = store.save();

        ensure_dreaming_cron(&mut store).unwrap();

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(job.schedule, "0 6 * * *");
        assert_eq!(job.jitter_max_ms, Some(1_800_000));
        // next_run must have moved off the legacy schedule.
        let new_next = job.next_run.expect("next_run must be set after migration");
        assert_ne!(
            new_next, stale_legacy_next,
            "migration must recompute next_run, not leave it anchored to the old schedule",
        );
        // And the new value must fit the new "0 6 * * *" cadence: minute-of-hour=0
        // plus 0..30min jitter window relative to UTC.
        let new_dt =
            chrono::DateTime::<chrono::Utc>::from_timestamp_millis(new_next as i64).unwrap();
        let minute = new_dt.minute();
        assert!(
            minute <= 30,
            "post-migration next_run must land in the 06:00..06:30 jitter window, got minute={minute}",
        );
    }

    #[test]
    fn custom_user_schedule_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        ensure_dreaming_cron(&mut store).unwrap();
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();
        if let Some(j) = store.get_mut(&id) {
            j.schedule = "30 12 * * *".into();
        }
        let _ = store.save();

        ensure_dreaming_cron(&mut store).unwrap();

        let job = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap();
        assert_eq!(
            job.schedule, "30 12 * * *",
            "user-customised schedule must NOT be overwritten"
        );
    }

    #[test]
    fn dream_cron_enabled_state_round_trips_through_store() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("crons.json");
        std::mem::forget(dir);
        let mut store = crate::cron::store::CronStore::new_with_path(path);

        ensure_dreaming_cron(&mut store).expect("create dream cron");
        let id = store
            .list()
            .iter()
            .find(|j| j.name == DREAMING_CRON_NAME)
            .unwrap()
            .id
            .clone();

        let enabled_now = |store: &crate::cron::store::CronStore, id: &str| -> bool {
            store.list().iter().find(|j| j.id == id).unwrap().enabled
        };

        assert!(
            enabled_now(&store, &id),
            "fresh dreaming cron must be enabled"
        );

        store.set_enabled(&id, false).unwrap();
        assert!(
            !enabled_now(&store, &id),
            "disable must persist through store API"
        );

        store.set_enabled(&id, true).unwrap();
        assert!(
            enabled_now(&store, &id),
            "re-enable must persist through store API"
        );
    }

    fn insert_entity(conn: &Connection, id: &str, etype: &str, age_days: i64) {
        let ts = (chrono::Utc::now() - chrono::Duration::days(age_days)).to_rfc3339();
        crate::mem::graph::insert_entity(
            conn,
            &crate::mem::graph::Entity {
                id: id.into(),
                entity_type: etype.into(),
                scope: "global".into(),
                properties: serde_json::json!({}),
            },
        )
        .unwrap();
        // Backdate so orphan-cleanup tests have an aged entity.
        conn.execute(
            "UPDATE entities SET created_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![&ts, id],
        )
        .unwrap();
    }

    fn insert_relation(conn: &Connection, from: &str, rel: &str, to: &str) {
        let now = chrono::Utc::now().to_rfc3339();
        crate::mem::graph::insert_relation(
            conn,
            &crate::mem::graph::Relation {
                from_id: from.into(),
                relation_type: rel.into(),
                to_id: to.into(),
                scope: "global".into(),
                valid_from: now,
                valid_to: None,
                confidence: 1.0,
                properties: serde_json::json!({}),
                reason: None,
            },
        )
        .unwrap();
    }

    #[tokio::test]
    async fn normalize_merges_high_confidence_asymmetric_pair() {
        // Typo-class pair: robert vs robertt. LD=1, ratio=6/7=0.857 ≥ 0.85.
        // robertt has 1 relation, robert has 3 → robertt becomes alias of robert.
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            for id in ["person_robert", "person_robertt", "x", "y", "z"] {
                let etype = if id.starts_with("person") {
                    "person"
                } else {
                    "thing"
                };
                insert_entity(&c, id, etype, 0);
            }
            insert_relation(&c, "person_robertt", "mentions", "x");
            for to in ["x", "y", "z"] {
                insert_relation(&c, "person_robert", "mentions", to);
            }
        }

        let report = normalize_phase(&mem).await.unwrap();
        assert_eq!(report.promoted, 1, "exactly one merge expected");

        let canonical: String = mem
            .conn()
            .lock()
            .query_row(
                "SELECT canonical_id FROM entity_aliases WHERE alias_id = 'person_robertt'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(canonical, "person_robert");
    }

    #[tokio::test]
    async fn normalize_ignores_nickname_pair_below_flag_threshold() {
        // Honest documentation: Levenshtein cannot catch nickname↔fullname
        // (tima vs timofey). Ratio ≈ 0.43 < 0.70 → ignored entirely.
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "person_tima", "person", 0);
            insert_entity(&c, "person_timofey", "person", 0);
        }
        normalize_phase(&mem).await.unwrap();
        let aliases: i64 = mem
            .conn()
            .lock()
            .query_row("SELECT COUNT(*) FROM entity_aliases", [], |r| r.get(0))
            .unwrap();
        assert_eq!(aliases, 0, "nickname-class pair must not be auto-merged");
    }

    #[tokio::test]
    async fn normalize_canonicalizes_synonymous_relations() {
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "e_a", "thing", 0);
            insert_entity(&c, "e_b", "thing", 0);
            insert_relation(&c, "e_a", "references", "e_b");
        }

        normalize_phase(&mem).await.unwrap();
        let kind: String = mem
            .conn()
            .lock()
            .query_row(
                "SELECT relation_type FROM relations WHERE from_id='e_a' AND to_id='e_b'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kind, "mentions");
    }

    #[tokio::test]
    async fn normalize_deletes_old_orphan_entities() {
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "orphan_1", "person", 20);
            insert_entity(&c, "kind_protected", "kind", 20);
        }

        normalize_phase(&mem).await.unwrap();
        let exists_orphan: i64 = mem
            .conn()
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id='orphan_1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        let exists_kind: i64 = mem
            .conn()
            .lock()
            .query_row(
                "SELECT COUNT(*) FROM entities WHERE id='kind_protected'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists_orphan, 0, "orphan must be deleted");
        assert_eq!(exists_kind, 1, "protected entity_type must survive");
    }

    #[tokio::test]
    async fn normalize_invalidates_broken_relations() {
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "e_a", "thing", 0);
            // Relation pointing to non-existent entity 'ghost'.
            insert_relation(&c, "e_a", "rel", "ghost");
        }

        normalize_phase(&mem).await.unwrap();
        let valid_to: Option<String> = mem
            .conn()
            .lock()
            .query_row(
                "SELECT valid_to FROM relations WHERE from_id='e_a' AND to_id='ghost'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(valid_to.is_some(), "broken relation must be invalidated");
    }

    #[tokio::test]
    async fn normalize_preserves_relations_anchored_in_mem_items() {
        let mem = MemManager::open_in_memory().unwrap();
        let fact_id = mem
            .save(NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "rust borrow checker".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "tech_rust", "tech", 0);
            insert_relation(&c, &fact_id, "mentions", "tech_rust");
        }

        normalize_phase(&mem).await.unwrap();
        let valid_to: Option<String> = mem
            .conn()
            .lock()
            .query_row(
                "SELECT valid_to FROM relations WHERE to_id='tech_rust'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            valid_to.is_none(),
            "fact_*→entity relation must survive normalize"
        );
    }

    #[tokio::test]
    async fn normalize_preserves_mem_item_to_mem_item_relations() {
        let mem = MemManager::open_in_memory().unwrap();
        let parent = mem
            .save(NewMemItem {
                kind: MemKind::Conversation,
                scope: MemScope::Global,
                title: "parent".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        let child = mem
            .save(NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: "child".into(),
                summary: None,
                area: None,
                tags: vec![],
                body: "x".into(),
                source: None,
                importance: 3.0,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_relation(&c, &child, "extracted_from", &parent);
        }

        normalize_phase(&mem).await.unwrap();
        let valid_to: Option<String> = mem
            .conn()
            .lock()
            .query_row(
                "SELECT valid_to FROM relations WHERE from_id=?1 AND to_id=?2",
                params![&child, &parent],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            valid_to.is_none(),
            "mem_item→mem_item relation must survive normalize"
        );
    }

    #[tokio::test]
    async fn normalize_flags_borderline_pair_without_merging() {
        let mem = MemManager::open_in_memory().unwrap();
        {
            let c = mem.conn();
            let c = c.lock();
            insert_entity(&c, "person_anya", "person", 0);
            insert_entity(&c, "person_anna", "person", 0);
        }

        normalize_phase(&mem).await.unwrap();
        let aliases: i64 = mem.conn().lock().query_row(
            "SELECT COUNT(*) FROM entity_aliases WHERE alias_id IN ('person_anya','person_anna')",
            [], |r| r.get(0),
        ).unwrap();
        assert_eq!(
            aliases, 0,
            "borderline pair (ratio < 0.92) must not be auto-merged"
        );
    }

    #[tokio::test]
    async fn deep_phase_keeps_high_importance_conversations() {
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
                source_path: None,
                pinned: false,
            })
            .unwrap();
        {
            let conn = mem.conn();
            let conn = conn.lock();
            let sixty_days_ago = (chrono::Utc::now() - chrono::Duration::days(60)).to_rfc3339();
            conn.execute(
                "UPDATE mem_items SET created_at = ?1 WHERE id = ?2",
                params![&sixty_days_ago, &id],
            )
            .unwrap();
        }

        let r = deep_phase(&mem).await.unwrap();
        assert_eq!(r.dropped, 0);
        assert!(mem.get(&id).unwrap().is_some());
    }
}
