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
    let truncated = if body.len() > 50_000 {
        let mut end = 50_000;
        while !body.is_char_boundary(end) { end -= 1; }
        &body[..end]
    } else {
        body
    };
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
    let entity_types: Vec<(String, u64)> = {
        let conn = svc.mem.conn().lock();
        conn.prepare("SELECT entity_type, COUNT(*) FROM entities GROUP BY entity_type ORDER BY COUNT(*) DESC LIMIT 20")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    };

    let relation_types: Vec<(String, u64)> = {
        let conn = svc.mem.conn().lock();
        conn.prepare("SELECT relation_type, COUNT(*) FROM relations GROUP BY relation_type ORDER BY COUNT(*) DESC LIMIT 30")
            .and_then(|mut stmt| {
                stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u64>(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    };

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

/// Find mem_items that are eligible for extraction but have no child items
/// linked back via `extracted_from`. These are either (a) never extracted,
/// or (b) extractions that produced zero child items. Caller decides.
pub fn find_pending_items(
    svc: &Arc<ServiceContext>,
    limit: usize,
) -> Result<Vec<(String, String, String, MemKind)>, String> {
    use crate::mem::store::MemKind::*;
    svc.mem.with_conn(|conn| {
        let mut stmt = conn.prepare(
            r#"SELECT m.id, m.kind, m.title, COALESCE(m.body_inline, '')
               FROM mem_items m
               WHERE m.kind IN ('conversation','note','daily','learning','fact')
                 AND m.tags_json NOT LIKE '%auto-extracted%'
                 AND NOT EXISTS (
                   SELECT 1 FROM relations r
                   WHERE r.to_id = m.id AND r.relation_type = 'extracted_from'
                 )
               ORDER BY m.created_at DESC
               LIMIT ?1"#,
        ).map_err(|e| e.to_string())?;

        let rows = stmt.query_map([limit as i64], |r| {
            let id: String = r.get(0)?;
            let kind_str: String = r.get(1)?;
            let title: String = r.get(2)?;
            let body: String = r.get(3)?;
            let kind = match kind_str.as_str() {
                "conversation" => Conversation,
                "note" => Note,
                "daily" => Daily,
                "learning" => Learning,
                "fact" => Fact,
                _ => Note,
            };
            Ok((id, title, body, kind))
        }).map_err(|e| e.to_string())?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| e.to_string())?);
        }
        Ok(out)
    })
}

/// Run extraction for pending items, sequentially spawning workers with
/// a short delay between each to avoid flooding the LLM provider. Returns
/// the number of workers queued. Runs in a background thread — returns immediately.
pub fn run_extract_pending(svc: Arc<ServiceContext>, limit: usize) -> usize {
    let pending = match find_pending_items(&svc, limit) {
        Ok(p) => p,
        Err(e) => {
            log::error!("[auto-extract] find_pending_items failed: {}", e);
            return 0;
        }
    };

    let count = pending.len();
    log::info!("[auto-extract] queued {} pending items for extraction", count);

    std::thread::spawn(move || {
        for (id, title, body, kind) in pending {
            spawn_extraction_worker(&svc, id, title, body, kind);
            // Small delay so we don't fork a dozen claude processes at once.
            std::thread::sleep(std::time::Duration::from_millis(500));
        }
    });

    count
}
