//! Heuristic re-clustering: hub detection, dominant-relation proposals,
//! apply-via-introduce_topic. Pure SQL reads + delegates mutations to
//! `graph::introduce_topic` so all writes go through one auditable path.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::MemStoreResult;
use crate::graph::{introduce_topic, IntroduceTopicOptions};
use crate::plan_format::{ClusterAction, ClusterPlan};

/// Active-edge degree fraction at which a single relation_type qualifies as
/// dominant for a hub. 40% — empirically matches the v1.1.22 manual reorg.
const DOMINANCE_RATIO: f32 = 0.40;

#[derive(Debug, Clone, Serialize)]
pub struct Hub {
    pub id: String,
    pub degree: u32,
}

#[derive(Debug, Clone, Serialize)]
pub struct ClusterCandidate {
    pub hub_id: String,
    pub dominant_rel: String,
    pub members: Vec<String>,
    pub ratio: f32,
    pub topic_id: String,
    pub anchor: String,
    pub rel_source: String,
    pub rel_anchor: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub clusters_applied: u64,
    pub clusters_failed: u64,
    pub errors: Vec<String>,
}

/// Active-edge degree of every node, sorted by degree desc, filtered by the
/// passed-in threshold. Counts each row twice (from + to) because hubs are
/// nodes that touch many edges in either direction.
pub fn find_hubs(conn: &Connection, threshold: u32) -> MemStoreResult<Vec<Hub>> {
    let mut stmt = conn.prepare(
        "SELECT id, COUNT(*) FROM (
            SELECT from_id AS id FROM relations WHERE valid_to IS NULL
            UNION ALL
            SELECT to_id AS id FROM relations WHERE valid_to IS NULL
         ) GROUP BY id HAVING COUNT(*) >= ?1 ORDER BY 2 DESC",
    )?;
    let rows = stmt.query_map(rusqlite::params![threshold], |r| {
        Ok(Hub {
            id: r.get::<_, String>(0)?,
            degree: r.get::<_, i64>(1)? as u32,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Group active incident edges of `hub` by relation_type. Any relation_type
/// whose count / total ≥ DOMINANCE_RATIO becomes a candidate cluster.
/// Members = the OTHER endpoint of each matching edge.
///
/// Topic-id selection prefers existing `topic_*` / `role_*` anchors per the
/// v1.1.22 hand-off notes — if the prefix shared by members already names
/// such a slug, we route into it; otherwise we synthesize
/// `topic_<prefix>_cluster_<n>`.
pub fn propose_for_hub(
    conn: &Connection,
    hub: &Hub,
) -> MemStoreResult<Vec<ClusterCandidate>> {
    let mut stmt = conn.prepare(
        "SELECT relation_type, from_id, to_id FROM relations \
         WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1)",
    )?;
    let rows = stmt.query_map(rusqlite::params![hub.id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
        ))
    })?;

    let mut by_rel: std::collections::HashMap<String, Vec<String>> = std::collections::HashMap::new();
    let mut total: u32 = 0;
    for r in rows {
        let (rel, from, to) = r?;
        let other = if from == hub.id { to } else { from };
        by_rel.entry(rel).or_default().push(other);
        total += 1;
    }

    if total == 0 {
        return Ok(Vec::new());
    }

    let existing_anchors = load_existing_topic_anchors(conn)?;

    let mut out = Vec::new();
    let mut sorted: Vec<(String, Vec<String>)> = by_rel.into_iter().collect();
    // Stable order: by relation_type ascending so output is deterministic.
    sorted.sort_by(|a, b| a.0.cmp(&b.0));

    for (rel, mut members) in sorted {
        let count = members.len() as u32;
        let ratio = count as f32 / total as f32;
        // Strictly above DOMINANCE_RATIO so a 40/60 split routes only one
        // side, not both. Matches the §4a.3 contract: "60% qualifies, 40%
        // does not".
        if ratio <= DOMINANCE_RATIO {
            continue;
        }
        members.sort();
        members.dedup();

        let topic_id = pick_topic_id(&hub.id, &rel, &members, &existing_anchors);

        out.push(ClusterCandidate {
            hub_id: hub.id.clone(),
            dominant_rel: rel.clone(),
            members,
            ratio,
            topic_id,
            anchor: hub.id.clone(),
            rel_source: rel.clone(),
            rel_anchor: rel,
        });
    }

    Ok(out)
}

fn load_existing_topic_anchors(conn: &Connection) -> MemStoreResult<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT id FROM entities \
         WHERE id LIKE 'topic_%' OR id LIKE 'role_%' \
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Pick a topic id for a candidate cluster.
///
/// 1. Compute the longest common underscore-segment prefix of `members`.
/// 2. If any existing `topic_*` / `role_*` slug shares that prefix or is a
///    plausible owner (its name minus the `topic_`/`role_` head matches the
///    members' theme), prefer it. Pin the §Hand-off invariant: load-bearing
///    slugs are reused, not regenerated.
/// 3. Otherwise synthesize a name. Using the hub id + relation_type
///    guarantees uniqueness across hubs in a single plan, so multiple hubs
///    sharing the same prefix don't collide on `topic_cluster_1`.
fn pick_topic_id(
    hub_id: &str,
    rel: &str,
    members: &[String],
    existing_anchors: &[String],
) -> String {
    let prefix = longest_common_underscore_prefix(members);

    if !prefix.is_empty() {
        // Direct match: an existing topic_* or role_* slug whose tail starts
        // with the same prefix wins.
        for anchor in existing_anchors {
            let tail = anchor
                .strip_prefix("topic_")
                .or_else(|| anchor.strip_prefix("role_"))
                .unwrap_or(anchor);
            if tail.starts_with(&prefix) || prefix.starts_with(tail) {
                return anchor.clone();
            }
        }
    }

    let hub_tail = hub_id.trim_start_matches("topic_").trim_start_matches("role_");
    let rel_slug = rel.replace(['-', ' '], "_");
    if prefix.is_empty() {
        format!("topic_{}_{}_cluster", hub_tail, rel_slug)
    } else if prefix.starts_with("topic_") || prefix.starts_with("role_") {
        // Don't double the prefix (e.g. avoid `topic_topic_release_*`).
        format!("{}_{}_cluster", prefix.trim_end_matches('_'), rel_slug)
    } else {
        format!("topic_{}_{}_cluster", prefix.trim_end_matches('_'), rel_slug)
    }
}

/// Longest shared prefix terminated at an underscore boundary across all ids.
fn longest_common_underscore_prefix(ids: &[String]) -> String {
    if ids.is_empty() {
        return String::new();
    }
    let first = &ids[0];
    let mut end = first.len();
    for other in &ids[1..] {
        let common = first
            .bytes()
            .zip(other.bytes())
            .take_while(|(a, b)| a == b)
            .count();
        end = end.min(common);
    }
    let candidate = &first[..end];
    // Trim back to last underscore so we don't cut inside a token.
    if let Some(idx) = candidate.rfind('_') {
        candidate[..idx].to_string()
    } else {
        String::new()
    }
}

/// Apply each cluster action via `graph::introduce_topic`. Per-cluster
/// failures are collected and reported; we never short-circuit the loop —
/// one bad cluster shouldn't block the rest.
pub fn apply(conn: &Connection, plan: &ClusterPlan) -> MemStoreResult<ApplyReport> {
    let mut applied: u64 = 0;
    let mut failed: u64 = 0;
    let mut errors: Vec<String> = Vec::new();

    for cluster in &plan.clusters {
        match apply_one(conn, cluster) {
            Ok(()) => applied += 1,
            Err(e) => {
                failed += 1;
                errors.push(format!("cluster {}: {}", cluster.topic_id, e));
            }
        }
    }

    Ok(ApplyReport {
        clusters_applied: applied,
        clusters_failed: failed,
        errors,
    })
}

fn apply_one(conn: &Connection, cluster: &ClusterAction) -> MemStoreResult<()> {
    let opts = IntroduceTopicOptions {
        topic_id: cluster.topic_id.clone(),
        entity_type: cluster.entity_type.clone(),
        sources: cluster.sources.clone(),
        anchor: cluster.anchor.clone(),
        rel_source: cluster.rel_source.clone(),
        rel_anchor: cluster.rel_anchor.clone(),
        reason: cluster.reason.clone(),
    };
    introduce_topic(conn, opts)?;
    Ok(())
}
