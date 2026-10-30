use crate::executor::Transaction;
use crate::{db_error, PostgresMemStore};
use codeterm_mem::dreaming::recluster::ReclusterReport;
use codeterm_mem_store::plan_format::{self, ClusterAction, ClusterPlan};
use codeterm_types::agent::models::resolve_model_id;
use serde::Deserialize;
use std::path::{Path, PathBuf};
const SUMMARY_TRUNC: usize = 120;
const MAX_CLUSTERS: usize = 5;
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

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max).collect();
    format!("{}…", cut)
}

fn build_prompt(hubs: &[HubInfo], anchors: &[AnchorInfo]) -> String {
    let mut s = String::new();
    s.push_str(
        "You are the CodeTerm Dreamer's recluster phase. The graph has the following hubs that may need reorganization. Existing topic/role anchors are listed — PREFER routing into them over inventing new slugs.\n\n",
    );
    s.push_str(&format!("Hubs (top {} by active degree):\n", hubs.len()));
    for (i, hub) in hubs.iter().enumerate() {
        s.push_str(&format!("{}. {} (degree {})\n", i + 1, hub.id, hub.degree));
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
            let title = if n.title.is_empty() {
                "(no title)"
            } else {
                &n.title
            };
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
    let start = trimmed
        .find('{')
        .ok_or_else(|| "no JSON object in response".to_string())?;
    let end = trimmed
        .rfind('}')
        .ok_or_else(|| "unterminated JSON object".to_string())?;
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

fn validate_cluster(conn: &mut Transaction<'_>, c: &RawCluster) -> Result<(), String> {
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
        return Err(format!(
            "cluster {}: anchor {} not in entities",
            c.topic_id, c.anchor
        ));
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

fn write_plan(plan_dir: &Path, timestamp: &str, plan: &ClusterPlan) -> Result<PathBuf, String> {
    std::fs::create_dir_all(plan_dir)
        .map_err(|e| format!("create_dir_all {}: {}", plan_dir.display(), e))?;
    let safe_ts = timestamp.replace(':', "").replace('.', "");
    let path = plan_dir.join(format!("{}-recluster.md", safe_ts));
    let body = plan_format::write(plan);
    std::fs::write(&path, body).map_err(|e| format!("write {}: {}", path.display(), e))?;
    Ok(path)
}

fn entity_exists(tx: &mut Transaction<'_>, id: &str) -> Result<bool, String> {
    Ok(tx
        .query_opt("SELECT id FROM entities WHERE id=$1", &[&id])
        .map_err(db_error)?
        .is_some())
}
fn mem_item_exists(tx: &mut Transaction<'_>, id: &str) -> Result<bool, String> {
    Ok(tx
        .query_opt("SELECT id FROM mem_items WHERE id=$1", &[&id])
        .map_err(db_error)?
        .is_some())
}
pub(super) fn prompt(tx: &mut Transaction<'_>) -> Result<Option<String>, String> {
    let rows=tx.query("SELECT e.id,(SELECT count(*) FROM relations r WHERE r.valid_to IS NULL AND (r.from_id=e.id OR r.to_id=e.id)) AS degree FROM entities e ORDER BY degree DESC,e.id", &[]).map_err(db_error)?;
    let mut hubs = Vec::new();
    for r in rows.into_iter().filter(|r| r.get::<_, i64>(1) > 0).take(10) {
        let id: String = r.get(0);
        let degree = r.get::<_, i64>(1) as u32;
        let relation_dist=tx.query("SELECT relation_type,count(*) FROM relations WHERE valid_to IS NULL AND (from_id=$1 OR to_id=$1) GROUP BY relation_type ORDER BY count(*) DESC,relation_type", &[&id]).map_err(db_error)?.into_iter().map(|r|(r.get(0),r.get::<_,i64>(1) as u32)).collect();
        let ids=tx.query("SELECT CASE WHEN from_id=$1 THEN to_id ELSE from_id END AS neighbor_id FROM relations WHERE valid_to IS NULL AND (from_id=$1 OR to_id=$1) GROUP BY neighbor_id ORDER BY count(*) DESC LIMIT 20", &[&id]).map_err(db_error)?;
        let mut neighbors = Vec::new();
        for row in ids {
            let id: String = row.get(0);
            let (title, summary) = if let Some(row) = tx
                .query_opt(
                    "SELECT title,coalesce(summary,'') FROM mem_items WHERE id=$1",
                    &[&id],
                )
                .map_err(db_error)?
            {
                (
                    row.get(0),
                    truncate(&row.get::<_, String>(1), SUMMARY_TRUNC),
                )
            } else if let Some(row) = tx
                .query_opt("SELECT properties_json FROM entities WHERE id=$1", &[&id])
                .map_err(db_error)?
            {
                let value = serde_json::from_str::<serde_json::Value>(&row.get::<_, String>(0))
                    .unwrap_or_default();
                (
                    value
                        .get("label")
                        .and_then(|v| v.as_str())
                        .map(|s| truncate(s, SUMMARY_TRUNC))
                        .unwrap_or_default(),
                    String::new(),
                )
            } else {
                (String::new(), String::new())
            };
            neighbors.push(NeighborInfo { id, title, summary });
        }
        hubs.push(HubInfo {
            id,
            degree,
            relation_dist,
            neighbors,
        });
    }
    let anchors=tx.query("SELECT id,entity_type FROM entities WHERE id LIKE 'topic_%' OR id LIKE 'role_%' ORDER BY id", &[]).map_err(db_error)?.into_iter().map(|r|AnchorInfo {id:r.get(0),entity_type:r.get(1)}).collect::<Vec<_>>();
    Ok((!hubs.is_empty()).then(|| build_prompt(&hubs, &anchors)))
}
pub(super) fn finalize(
    store: &PostgresMemStore,
    response: &str,
    model: &str,
    dir: &Path,
) -> Result<ReclusterReport, String> {
    let clusters = parse_response(response)?;
    let timestamp = chrono::Utc::now().to_rfc3339();
    let (actions, warnings) = store.transaction(|tx| {
        let mut actions = Vec::new();
        let mut warnings = Vec::new();
        for raw in clusters.into_iter().take(MAX_CLUSTERS) {
            match validate_cluster(tx, &raw) {
                Ok(()) => actions.push(to_action(raw, &timestamp)),
                Err(e) => warnings.push(e),
            }
        }
        Ok((actions, warnings))
    })?;
    let plan = ClusterPlan {
        generated_at: timestamp.clone(),
        clusters: actions,
    };
    let path = write_plan(dir, &timestamp, &plan)?;
    let mut message = format!(
        "wrote {} cluster(s) to {}",
        plan.clusters.len(),
        path.display()
    );
    if !warnings.is_empty() {
        message.push_str(" — warnings: ");
        message.push_str(&warnings.join("; "));
    }
    Ok(ReclusterReport {
        plan_path: Some(path.display().to_string()),
        clusters_proposed: plan.clusters.len() as u32,
        model_used: resolve_model_id(model).to_owned(),
        message,
    })
}
