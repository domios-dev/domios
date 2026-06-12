//! Markdown-with-YAML-fences plan format. The contract Track 4a (heuristic)
//! and any future Track 4b (LLM) emit and consume — see §Decisions D2.
//!
//! Schema is fixed: a generated_at header, then per-cluster sections each
//! containing a rationale paragraph plus a fenced ```yaml block with a few
//! string fields and a `sources:` list. We hand-parse to keep mem-store deps
//! tight (no serde_yaml).

use serde::Serialize;

use crate::error::{MemStoreError, MemStoreResult};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterPlan {
    pub generated_at: String,
    pub clusters: Vec<ClusterAction>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ClusterAction {
    pub topic_id: String,
    pub entity_type: String,
    pub anchor: String,
    pub rel_source: String,
    pub rel_anchor: String,
    pub reason: Option<String>,
    pub rationale: String,
    pub sources: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ParseReport {
    pub plan: ClusterPlan,
    pub warnings: Vec<String>,
}

/// Render a ClusterPlan as markdown the apply step (and a human reviewer)
/// can consume. Each cluster gets its own H3 section + a yaml fence.
pub fn write(plan: &ClusterPlan) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "# Re-cluster proposal — generated {}\n\n",
        plan.generated_at
    ));
    if plan.clusters.is_empty() {
        out.push_str("_No candidate clusters found. Lower the threshold to widen the search._\n");
        return out;
    }
    for cluster in &plan.clusters {
        out.push_str(&format!("### Cluster: {}\n\n", cluster.topic_id));
        if !cluster.rationale.trim().is_empty() {
            out.push_str(&format!("Rationale: {}\n\n", cluster.rationale));
        }
        out.push_str("```yaml\n");
        out.push_str("action: introduce_topic\n");
        out.push_str(&format!("topic_id: {}\n", cluster.topic_id));
        out.push_str(&format!("entity_type: {}\n", cluster.entity_type));
        out.push_str(&format!("anchor: {}\n", cluster.anchor));
        out.push_str(&format!("rel_source: {}\n", cluster.rel_source));
        out.push_str(&format!("rel_anchor: {}\n", cluster.rel_anchor));
        if let Some(reason) = &cluster.reason {
            out.push_str(&format!("reason: {}\n", reason));
        }
        out.push_str("sources:\n");
        for src in &cluster.sources {
            out.push_str(&format!("  - {}\n", src));
        }
        out.push_str("```\n\n");
    }
    out
}

/// Parse a markdown plan back into a ClusterPlan. Malformed individual
/// fences are skipped with a warning written to ParseReport.warnings;
/// whole-file errors (missing/unparseable header) abort with Err.
pub fn read(md: &str) -> MemStoreResult<ParseReport> {
    let generated_at = extract_generated_at(md).ok_or_else(|| {
        MemStoreError::InvalidArgument(
            "plan markdown missing '# Re-cluster proposal — generated <ISO>' header".into(),
        )
    })?;

    let mut clusters = Vec::new();
    let mut warnings = Vec::new();

    for fence in iter_yaml_fences(md) {
        match parse_cluster_yaml(&fence.body) {
            Ok(Some(cluster)) => {
                let mut cluster = cluster;
                if cluster.rationale.is_empty() {
                    cluster.rationale = fence.rationale.clone();
                }
                clusters.push(cluster);
            }
            Ok(None) => {
                // Not an introduce_topic action — silently skip; future
                // action types should appear here without breaking parsing.
            }
            Err(e) => warnings.push(format!("skipped cluster fence: {}", e)),
        }
    }

    Ok(ParseReport {
        plan: ClusterPlan {
            generated_at,
            clusters,
        },
        warnings,
    })
}

fn extract_generated_at(md: &str) -> Option<String> {
    for line in md.lines() {
        if let Some(rest) = line.strip_prefix("# Re-cluster proposal") {
            // Accept either "— generated <ts>" or "- generated <ts>" or
            // "generated <ts>" so format drift over time still parses.
            let cleaned = rest
                .trim_start_matches(|c: char| c == ' ' || c == '—' || c == '-')
                .trim();
            if let Some(stripped) = cleaned.strip_prefix("generated ") {
                return Some(stripped.trim().to_string());
            }
            if cleaned.is_empty() {
                return Some(String::new());
            }
        }
    }
    None
}

struct YamlFence {
    body: String,
    rationale: String,
}

/// Walk markdown looking for ```yaml ... ``` fences. For each one, capture
/// the rationale paragraph that immediately precedes the fence inside the
/// same `### Cluster:` section.
fn iter_yaml_fences(md: &str) -> Vec<YamlFence> {
    let mut fences = Vec::new();
    let mut current_rationale = String::new();
    let mut in_fence = false;
    let mut fence_body = String::new();

    for line in md.lines() {
        let trimmed = line.trim_start();
        if !in_fence {
            if trimmed.starts_with("```yaml") || trimmed == "```yml" {
                in_fence = true;
                fence_body.clear();
                continue;
            }
            if let Some(rest) = trimmed.strip_prefix("Rationale:") {
                current_rationale = rest.trim().to_string();
            } else if line.starts_with("### Cluster:") {
                current_rationale.clear();
            } else if !trimmed.is_empty() && !current_rationale.is_empty() {
                // Multi-line rationale: keep accumulating until a blank line.
                if !line.starts_with("```") {
                    current_rationale.push(' ');
                    current_rationale.push_str(line.trim());
                }
            } else if trimmed.is_empty() {
                // Blank line ends rationale accumulation. Don't reset
                // current_rationale yet — the fence about to open should still
                // see it, but a fresh ### Cluster: will reset on its own.
            }
        } else if trimmed == "```" {
            fences.push(YamlFence {
                body: std::mem::take(&mut fence_body),
                rationale: std::mem::take(&mut current_rationale),
            });
            in_fence = false;
        } else {
            fence_body.push_str(line);
            fence_body.push('\n');
        }
    }

    fences
}

/// Hand-parse one yaml fence body. Returns None when `action` is set but is
/// not `introduce_topic` (forward-compat for new action verbs). Returns Err
/// when the fence is corrupt enough that we shouldn't quietly drop it.
fn parse_cluster_yaml(body: &str) -> Result<Option<ClusterAction>, String> {
    let mut action = String::new();
    let mut topic_id = String::new();
    let mut entity_type = String::new();
    let mut anchor = String::new();
    let mut rel_source = String::new();
    let mut rel_anchor = String::new();
    let mut reason: Option<String> = None;
    let mut sources: Vec<String> = Vec::new();
    let mut in_sources = false;

    for raw_line in body.lines() {
        let line = raw_line.trim_end();
        if line.trim().is_empty() {
            continue;
        }
        if in_sources {
            let stripped = line.trim_start();
            if let Some(rest) = stripped.strip_prefix("- ") {
                sources.push(rest.trim().to_string());
                continue;
            }
            // Non-list line ends the sources block.
            in_sources = false;
        }
        if line.starts_with("sources:") {
            in_sources = true;
            continue;
        }
        let (key, value) = split_kv(line).ok_or_else(|| format!("malformed line: {:?}", line))?;
        match key.as_str() {
            "action" => action = value,
            "topic_id" => topic_id = value,
            "entity_type" => entity_type = value,
            "anchor" => anchor = value,
            "rel_source" => rel_source = value,
            "rel_anchor" => rel_anchor = value,
            "reason" => reason = if value.is_empty() { None } else { Some(value) },
            // Unknown keys are ignored — the format is open for additions.
            _ => {}
        }
    }

    if action.is_empty() {
        return Err("missing `action` field".into());
    }
    if action != "introduce_topic" {
        return Ok(None);
    }
    if topic_id.is_empty() || anchor.is_empty() {
        return Err("introduce_topic requires topic_id and anchor".into());
    }
    if entity_type.is_empty() {
        entity_type = "Topic".into();
    }
    if rel_source.is_empty() {
        rel_source = "about".into();
    }
    if rel_anchor.is_empty() {
        rel_anchor = "about".into();
    }
    if sources.is_empty() {
        return Err("introduce_topic requires at least one source".into());
    }

    Ok(Some(ClusterAction {
        topic_id,
        entity_type,
        anchor,
        rel_source,
        rel_anchor,
        reason,
        rationale: String::new(),
        sources,
    }))
}

fn split_kv(line: &str) -> Option<(String, String)> {
    let trimmed = line.trim_start();
    let idx = trimmed.find(':')?;
    let key = trimmed[..idx].trim().to_string();
    let value = trimmed[idx + 1..].trim().to_string();
    if key.is_empty() {
        return None;
    }
    Some((key, value))
}
