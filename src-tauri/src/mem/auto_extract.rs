//! Hidden graph-worker that extracts entities/relations from mem_items.

use std::sync::Arc;

use crate::mem::store::MemKind;
use crate::mem::workers;
use crate::service::ServiceContext;

fn should_extract(kind: MemKind) -> bool {
    !matches!(kind, MemKind::Summary)
}

fn build_extraction_prompt(
    svc: &ServiceContext,
    mem_item_id: &str,
    title: &str,
    body: &str,
) -> String {
    let truncated = if body.len() > 50_000 { &body[..50_000] } else { body };
    let areas_block = format_existing_areas(svc);
    let graph_types_block = format_existing_graph_types(svc);
    let dedup_threshold = crate::config::memory_config().dedup_threshold;
    crate::prompts::MEM_EXTRACTION
        .replace("{id}", mem_item_id)
        .replace("{title}", title)
        .replace("{body}", truncated)
        .replace("{existing_areas}", &areas_block)
        .replace("{existing_graph_types}", &graph_types_block)
        .replace("{dedup_threshold}", &format!("{:.2}", dedup_threshold))
}

// Injects the live taxonomy so the worker picks from areas that actually
// exist in mem (ordered by usage) instead of from a hardcoded table that
// drifts from reality. Worker is read-only on areas: pick one of these or
// leave empty. Only the main agent creates new areas.
fn format_existing_areas(svc: &ServiceContext) -> String {
    let tax = match svc.mem.get_taxonomy(None) {
        Ok(t) => t,
        Err(_) => return "(none yet — use \"\" if unsure)".to_string(),
    };
    let mut rows: Vec<(String, u64)> = tax
        .into_iter()
        .filter(|(area, _)| area != "(none)" && !area.is_empty())
        .map(|(area, by_kind)| {
            let total: u64 = by_kind.values().sum();
            (area, total)
        })
        .collect();
    if rows.is_empty() {
        return "(none yet — use \"\" if unsure)".to_string();
    }
    rows.sort_by(|a, b| b.1.cmp(&a.1));
    rows.truncate(12);
    rows.into_iter()
        .map(|(a, n)| format!("- {} ({} items)", a, n))
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_existing_graph_types(svc: &ServiceContext) -> String {
    let conn = svc.mem.conn().lock();

    let entity_types: Vec<(String, u64)> = conn
        .prepare("SELECT entity_type, COUNT(*) FROM entities GROUP BY entity_type ORDER BY COUNT(*) DESC LIMIT 20")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .unwrap_or_default();

    let relation_types: Vec<(String, u64)> = conn
        .prepare("SELECT relation_type, COUNT(*) FROM relations GROUP BY relation_type ORDER BY COUNT(*) DESC LIMIT 30")
        .and_then(|mut stmt| {
            stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
        })
        .unwrap_or_default();

    let mut out = String::new();

    if entity_types.is_empty() {
        out.push_str("Entity types: (none yet — common types: Person, Project, Tech, Machine)\n");
    } else {
        out.push_str("Entity types in use:\n");
        for (t, n) in &entity_types {
            out.push_str(&format!("- {} ({} entities)\n", t, n));
        }
    }

    if relation_types.is_empty() {
        out.push_str("Relation types: (none yet — use verb-based: works_on, uses, shipped, runs_on, mentions)");
    } else {
        out.push_str("Relation types in use:\n");
        for (t, n) in &relation_types {
            out.push_str(&format!("- {} ({} relations)\n", t, n));
        }
    }

    out
}

pub fn spawn_extraction_worker(
    svc: &Arc<ServiceContext>,
    mem_item_id: String,
    title: String,
    body: String,
    kind: MemKind,
) -> Option<String> {
    if !should_extract(kind) {
        return None;
    }

    let prompt = build_extraction_prompt(svc, &mem_item_id, &title, &body);

    let mem_cfg = crate::config::memory_config();
    let agent_provider = if mem_cfg.extraction_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.extraction_agent.clone()
    };
    // Extraction workers use `extraction_model` when set, otherwise fall
    // back to the shared `worker_model` default (claude-haiku-4-5-*).
    let model = if mem_cfg.extraction_model.is_empty() {
        mem_cfg.worker_model.clone()
    } else {
        mem_cfg.extraction_model.clone()
    };

    let intent = format!("extract:{}", mem_item_id);
    match workers::spawn_headless(
        svc,
        intent,
        prompt,
        "auto:save".into(),
        agent_provider,
        model,
    ) {
        Ok(worker_id) => {
            log::info!(
                "[auto-extract] headless worker {} for {}",
                worker_id, mem_item_id
            );
            Some(worker_id)
        }
        Err(e) => {
            log::warn!("[auto-extract] spawn failed for {}: {}", mem_item_id, e);
            None
        }
    }
}
