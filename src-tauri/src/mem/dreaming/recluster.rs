//! LLM-driven re-cluster phase. Read-only against the graph, writes a
//! markdown plan to `~/.codeterm/mem/dream-plans/<utc-ts>-recluster.md`.
//! User reviews and applies via `mem dream apply <file>`.
//!
//! Opt-in only — never invoked by the cron-driven Light/Deep/Normalize
//! pipeline. Reachable via explicit `mem dream run --phase recluster`.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use codeterm_mem_store::plan_format::{self, ClusterAction, ClusterPlan};

const TOP_HUBS: usize = 10;
const NEIGHBORS_PER_HUB: usize = 20;
const SUMMARY_TRUNC: usize = 120;
const MAX_CLUSTERS: usize = 5;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReclusterReport {
    pub plan_path: Option<String>,
    pub clusters_proposed: u32,
    pub model_used: String,
    pub message: String,
}

/// Boundary for the LLM call so tests can inject a deterministic stub.
#[async_trait]
pub trait LlmCaller: Send + Sync {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, String>;
}

/// Production caller — Anthropic /v1/messages. Reads the API key from
/// `ANTHROPIC_API_KEY` (matches the rerank pathway).
pub struct AnthropicCaller;

#[async_trait]
impl LlmCaller for AnthropicCaller {
    async fn complete(&self, prompt: &str, model: &str) -> Result<String, String> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| "ANTHROPIC_API_KEY not set — the recluster phase needs an Anthropic key".to_string())?;
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .map_err(|e| e.to_string())?;
        let body = serde_json::json!({
            "model": model,
            "max_tokens": 2048,
            "messages": [{"role": "user", "content": prompt}],
        });
        let resp = client
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| e.to_string())?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("anthropic {}: {}", status, body));
        }
        let val: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
        let text = val
            .get("content")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|m| m.get("text"))
            .and_then(|t| t.as_str())
            .ok_or_else(|| "missing content[0].text".to_string())?;
        Ok(text.to_string())
    }
}

#[derive(Debug, Clone)]
struct HubInfo {
    id: String,
    degree: u32,
    relation_dist: Vec<(String, u32)>,
    neighbors: Vec<NeighborInfo>,
}

#[derive(Debug, Clone)]
struct NeighborInfo {
    id: String,
    title: String,
    summary: String,
}

#[derive(Debug, Clone)]
struct AnchorInfo {
    id: String,
    entity_type: String,
}

/// Run the recluster phase end-to-end. Production callers pass the real
/// `AnthropicCaller`; tests pass a stub that returns a fixed JSON string.
///
/// Test convenience wrapper that takes a `&Connection`. Production code
/// goes through `run_recluster_phase_with_lock` so the conn lock is
/// released across the LLM call. Test connections are not behind a mutex.
pub async fn run_recluster_phase(
    conn: &Connection,
    dreaming_model: &str,
    plan_dir: &Path,
    caller: &dyn LlmCaller,
) -> Result<ReclusterReport, String> {
    let hubs = collect_hubs(conn, TOP_HUBS, NEIGHBORS_PER_HUB)?;
    let anchors = collect_anchors(conn)?;

    if hubs.is_empty() {
        return Ok(ReclusterReport {
            plan_path: None,
            clusters_proposed: 0,
            model_used: dreaming_model.to_string(),
            message: "no hubs found — graph is empty or all entities orphaned".into(),
        });
    }

    let prompt = build_prompt(&hubs, &anchors);
    let resolved = crate::agent::models::resolve_model_id(dreaming_model);
    let raw_response = caller.complete(&prompt, resolved).await?;
    let parsed_clusters = parse_response(&raw_response)?;

    finalize_recluster(conn, parsed_clusters, dreaming_model, resolved, plan_dir)
}

/// Production entry: takes the parking_lot-locked mem connection, drops
/// the lock across the LLM call, re-acquires it for validation + write.
/// Keeps the DB free for other workers during the network-bound phase.
pub async fn run_recluster_phase_with_lock(
    conn_mutex: &parking_lot::Mutex<Connection>,
    dreaming_model: &str,
    plan_dir: &Path,
    caller: &dyn LlmCaller,
) -> Result<ReclusterReport, String> {
    let (hubs, anchors) = {
        let c = conn_mutex.lock();
        let hubs = collect_hubs(&c, TOP_HUBS, NEIGHBORS_PER_HUB)?;
        let anchors = collect_anchors(&c)?;
        (hubs, anchors)
    };

    if hubs.is_empty() {
        return Ok(ReclusterReport {
            plan_path: None,
            clusters_proposed: 0,
            model_used: dreaming_model.to_string(),
            message: "no hubs found — graph is empty or all entities orphaned".into(),
        });
    }

    let prompt = build_prompt(&hubs, &anchors);
    let resolved = crate::agent::models::resolve_model_id(dreaming_model);
    let raw_response = caller.complete(&prompt, resolved).await?;
    let parsed_clusters = parse_response(&raw_response)?;

    let c = conn_mutex.lock();
    finalize_recluster(&c, parsed_clusters, dreaming_model, resolved, plan_dir)
}

fn finalize_recluster(
    conn: &Connection,
    parsed_clusters: Vec<RawCluster>,
    _dreaming_model: &str,
    resolved: &str,
    plan_dir: &Path,
) -> Result<ReclusterReport, String> {
    let mut warnings: Vec<String> = Vec::new();
    let mut actions: Vec<ClusterAction> = Vec::new();
    let timestamp = chrono::Utc::now().to_rfc3339();
    for raw in parsed_clusters.into_iter().take(MAX_CLUSTERS) {
        match validate_cluster(conn, &raw) {
            Ok(()) => actions.push(to_action(raw, &timestamp)),
            Err(e) => warnings.push(e),
        }
    }

    let plan = ClusterPlan {
        generated_at: timestamp.clone(),
        clusters: actions,
    };

    let path = write_plan(plan_dir, &timestamp, &plan)?;

    // Best-effort Telegram ping. Crashing here would defeat the point.
    let _ = try_bridge_ping(&path, plan.clusters.len() as u32, &warnings);

    let mut message = format!(
        "wrote {} cluster(s) to {}",
        plan.clusters.len(),
        path.display(),
    );
    if !warnings.is_empty() {
        message.push_str(" — warnings: ");
        message.push_str(&warnings.join("; "));
    }

    Ok(ReclusterReport {
        plan_path: Some(path.display().to_string()),
        clusters_proposed: plan.clusters.len() as u32,
        model_used: resolved.to_string(),
        message,
    })
}

#[derive(Debug, Clone, Deserialize)]
struct RawCluster {
    topic_id: String,
    #[serde(default)]
    entity_type: Option<String>,
    anchor: String,
    sources: Vec<String>,
    #[serde(default)]
    rationale: Option<String>,
}

fn collect_hubs(
    conn: &Connection,
    top_n: usize,
    neighbors_per_hub: usize,
) -> Result<Vec<HubInfo>, String> {
    let entities = codeterm_mem_store::graph::list_entities_with_degree(conn)
        .map_err(|e| format!("list_entities_with_degree: {}", e))?;
    let mut sorted: Vec<_> = entities.into_iter().filter(|e| e.degree > 0).collect();
    sorted.sort_by(|a, b| b.degree.cmp(&a.degree).then_with(|| a.id.cmp(&b.id)));
    let top: Vec<_> = sorted.into_iter().take(top_n).collect();

    let mut hubs = Vec::new();
    for entry in top {
        let dist = relation_distribution(conn, &entry.id)?;
        let neighbors = neighbors_for(conn, &entry.id, neighbors_per_hub)?;
        hubs.push(HubInfo {
            id: entry.id,
            degree: entry.degree,
            relation_dist: dist,
            neighbors,
        });
    }
    Ok(hubs)
}

fn relation_distribution(conn: &Connection, hub_id: &str) -> Result<Vec<(String, u32)>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT relation_type, COUNT(*) FROM relations \
             WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1) \
             GROUP BY relation_type ORDER BY COUNT(*) DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![hub_id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn neighbors_for(
    conn: &Connection,
    hub_id: &str,
    limit: usize,
) -> Result<Vec<NeighborInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT CASE WHEN from_id = ?1 THEN to_id ELSE from_id END AS neighbor_id \
             FROM relations \
             WHERE valid_to IS NULL AND (from_id = ?1 OR to_id = ?1) \
             GROUP BY neighbor_id \
             ORDER BY COUNT(*) DESC \
             LIMIT ?2",
        )
        .map_err(|e| e.to_string())?;
    let neighbor_ids: Vec<String> = stmt
        .query_map(params![hub_id, limit as i64], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    let mut out = Vec::new();
    for nid in neighbor_ids {
        let info = lookup_neighbor(conn, &nid)?;
        out.push(info);
    }
    Ok(out)
}

fn lookup_neighbor(conn: &Connection, id: &str) -> Result<NeighborInfo, String> {
    if let Some((title, summary)) = lookup_mem_item(conn, id)? {
        return Ok(NeighborInfo {
            id: id.to_string(),
            title,
            summary,
        });
    }
    if let Some(label) = lookup_entity_label(conn, id)? {
        return Ok(NeighborInfo {
            id: id.to_string(),
            title: label,
            summary: String::new(),
        });
    }
    Ok(NeighborInfo {
        id: id.to_string(),
        title: String::new(),
        summary: String::new(),
    })
}

fn lookup_mem_item(conn: &Connection, id: &str) -> Result<Option<(String, String)>, String> {
    let mut stmt = conn
        .prepare("SELECT title, COALESCE(summary, '') FROM mem_items WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.next() {
        let (title, summary) = row.map_err(|e| e.to_string())?;
        return Ok(Some((title, truncate(&summary, SUMMARY_TRUNC))));
    }
    Ok(None)
}

fn lookup_entity_label(conn: &Connection, id: &str) -> Result<Option<String>, String> {
    let mut stmt = conn
        .prepare("SELECT properties_json FROM entities WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let mut rows = stmt
        .query_map(params![id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    if let Some(row) = rows.next() {
        let raw = row.map_err(|e| e.to_string())?;
        let val: serde_json::Value = serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        let label = val
            .get("label")
            .and_then(|v| v.as_str())
            .map(|s| truncate(s, SUMMARY_TRUNC))
            .unwrap_or_default();
        return Ok(Some(label));
    }
    Ok(None)
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut)
}

fn collect_anchors(conn: &Connection) -> Result<Vec<AnchorInfo>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, entity_type FROM entities \
             WHERE id LIKE 'topic_%' OR id LIKE 'role_%' \
             ORDER BY id",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AnchorInfo {
                id: r.get::<_, String>(0)?,
                entity_type: r.get::<_, String>(1)?,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r.map_err(|e| e.to_string())?);
    }
    Ok(out)
}

fn build_prompt(hubs: &[HubInfo], anchors: &[AnchorInfo]) -> String {
    let mut s = String::new();
    s.push_str(
        "You are the CodeTerm Dreamer's recluster phase. The graph has the following hubs that may need reorganization. Existing topic/role anchors are listed — PREFER routing into them over inventing new slugs.\n\n",
    );
    s.push_str(&format!("Hubs (top {} by active degree):\n", hubs.len()));
    for (i, hub) in hubs.iter().enumerate() {
        s.push_str(&format!(
            "{}. {} (degree {})\n",
            i + 1,
            hub.id,
            hub.degree
        ));
        let dist = if hub.relation_dist.is_empty() {
            "(none)".to_string()
        } else {
            hub.relation_dist
                .iter()
                .map(|(rel, n)| format!("{}: {}", rel, n))
                .collect::<Vec<_>>()
                .join(", ")
        };
        s.push_str(&format!("   relation type distribution: {}\n", dist));
        s.push_str(&format!("   neighbors (top {}):\n", hub.neighbors.len()));
        for n in &hub.neighbors {
            let title = if n.title.is_empty() { "(no title)" } else { &n.title };
            if n.summary.is_empty() {
                s.push_str(&format!("   - {}: {}\n", n.id, title));
            } else {
                s.push_str(&format!("   - {}: {} — {}\n", n.id, title, n.summary));
            }
        }
    }

    s.push_str("\nExisting anchors (use these as targets when content matches):\n");
    if anchors.is_empty() {
        s.push_str("- (none — propose new topic_* slugs)\n");
    } else {
        for a in anchors {
            s.push_str(&format!("- {} ({})\n", a.id, a.entity_type));
        }
    }

    s.push_str(&format!(
        "\nPropose 0 to {MAX_CLUSTERS} cluster operations. Each operation: pick a hub whose neighbors clearly fall into a coherent group, route the group through a topic intermediate (existing or new), invalidating the direct hub→neighbor edges.\n\n",
    ));
    s.push_str(
        "Respond with strict JSON ONLY (no prose, no fences):\n\
         {\n  \"clusters\": [\n    {\n      \"topic_id\": \"topic_<slug>\",\n      \"entity_type\": \"Topic\" | \"Role\",\n      \"anchor\": \"<hub_id>\",\n      \"sources\": [\"<neighbor_id>\", ...],\n      \"rationale\": \"<one sentence>\"\n    }\n  ]\n}\n\n\
         If nothing should change, return {\"clusters\": []}.\n",
    );
    s
}

fn parse_response(raw: &str) -> Result<Vec<RawCluster>, String> {
    let trimmed = raw.trim();
    let start = trimmed.find('{').ok_or_else(|| "no JSON object in response".to_string())?;
    let end = trimmed.rfind('}').ok_or_else(|| "unterminated JSON object".to_string())?;
    let json = &trimmed[start..=end];
    let val: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("parse JSON: {}", e))?;
    let arr = val
        .get("clusters")
        .and_then(|v| v.as_array())
        .ok_or_else(|| "missing 'clusters' array".to_string())?;
    let mut out = Vec::new();
    for c in arr {
        let raw: RawCluster =
            serde_json::from_value(c.clone()).map_err(|e| format!("cluster shape: {}", e))?;
        out.push(raw);
    }
    Ok(out)
}

fn validate_cluster(conn: &Connection, c: &RawCluster) -> Result<(), String> {
    if !is_valid_slug(&c.topic_id) {
        return Err(format!("invalid topic_id slug: {:?}", c.topic_id));
    }
    if c.sources.is_empty() {
        return Err(format!("cluster {} has empty sources", c.topic_id));
    }
    if c.anchor == c.topic_id {
        return Err(format!("cluster {} anchor equals topic_id", c.topic_id));
    }
    if !entity_exists(conn, &c.anchor)? {
        return Err(format!("cluster {}: anchor {} not in entities", c.topic_id, c.anchor));
    }
    for src in &c.sources {
        if src == &c.anchor {
            return Err(format!(
                "cluster {} has source equal to anchor: {}",
                c.topic_id, src
            ));
        }
        if src == &c.topic_id {
            return Err(format!(
                "cluster {} has source equal to topic_id: {}",
                c.topic_id, src
            ));
        }
        if !entity_exists(conn, src)? && !mem_item_exists(conn, src)? {
            return Err(format!(
                "cluster {}: source {} missing from entities and mem_items",
                c.topic_id, src
            ));
        }
    }
    Ok(())
}

fn is_valid_slug(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn entity_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM entities WHERE id = ?1",
        params![id],
        |_| Ok(()),
    )
    .map(|_| true)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(other.to_string()),
    })
}

fn mem_item_exists(conn: &Connection, id: &str) -> Result<bool, String> {
    conn.query_row(
        "SELECT 1 FROM mem_items WHERE id = ?1",
        params![id],
        |_| Ok(()),
    )
    .map(|_| true)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(false),
        other => Err(other.to_string()),
    })
}

fn to_action(raw: RawCluster, timestamp: &str) -> ClusterAction {
    ClusterAction {
        topic_id: raw.topic_id,
        entity_type: raw.entity_type.unwrap_or_else(|| "Topic".into()),
        anchor: raw.anchor,
        rel_source: "about".into(),
        rel_anchor: "about".into(),
        reason: Some(format!("dreamer-recluster-{}", timestamp)),
        rationale: raw.rationale.unwrap_or_default(),
        sources: raw.sources,
    }
}

fn write_plan(
    plan_dir: &Path,
    timestamp: &str,
    plan: &ClusterPlan,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(plan_dir)
        .map_err(|e| format!("create_dir_all {}: {}", plan_dir.display(), e))?;
    let safe_ts = timestamp.replace(':', "").replace('.', "");
    let path = plan_dir.join(format!("{}-recluster.md", safe_ts));
    let body = plan_format::write(plan);
    std::fs::write(&path, body).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

fn try_bridge_ping(path: &Path, n_clusters: u32, warnings: &[String]) -> Result<(), String> {
    let mut text = format!(
        "[dreamer] recluster plan ready — {} cluster(s) at {}",
        n_clusters,
        path.display()
    );
    if !warnings.is_empty() {
        text.push_str(&format!(" ({} warning(s))", warnings.len()));
    }
    let cli = std::env::var("CODETERM_CLI_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("codeterm"));
    let _ = std::process::Command::new(cli)
        .args(["bridge", "send", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            child.wait()
        });
    Ok(())
}

#[cfg(test)]
mod tests;
