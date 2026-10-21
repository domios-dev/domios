//! Re-cluster apply pipeline. The plan-format parser produces a
//! `ClusterPlan`; this module routes each `ClusterAction` through
//! `graph::introduce_topic` so all writes go through one auditable path.
//!
//! The heuristic phase that originally generated these plans (hub detection,
//! dominant-relation proposals) was stripped before v1.1.23 ship — it could
//! only group by relation_type frequency and missed the semantic structure a
//! human or LLM produces. A proper Dreamer-driven re-cluster phase lands in
//! v1.1.24 and emits the same markdown plan format this module consumes.

use rusqlite::Connection;
use serde::Serialize;

use crate::error::MemStoreResult;
use crate::graph::{introduce_topic, IntroduceTopicOptions};
use crate::plan_format::{ClusterAction, ClusterPlan};

#[derive(Debug, Clone, Serialize)]
pub struct ApplyReport {
    pub clusters_applied: u64,
    pub clusters_failed: u64,
    pub errors: Vec<String>,
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
