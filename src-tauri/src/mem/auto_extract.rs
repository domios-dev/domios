//! Hidden graph-worker that extracts entities/relations from mem_items.

use std::sync::Arc;

use crate::mem::store::MemKind;
use crate::mem::workers;
use crate::service::ServiceContext;

fn should_extract(kind: MemKind) -> bool {
    matches!(
        kind,
        MemKind::Conversation | MemKind::Note | MemKind::Daily | MemKind::Learning | MemKind::Fact
    )
}

const MIN_BODY_LEN: usize = 80;

fn build_extraction_prompt(
    svc: &ServiceContext,
    mem_item_id: &str,
    title: &str,
    body: &str,
) -> String {
    let truncated = if body.len() > 4000 { &body[..4000] } else { body };
    let areas_block = format_existing_areas(svc);
    crate::prompts::MEM_EXTRACTION
        .replace("{id}", mem_item_id)
        .replace("{title}", title)
        .replace("{body}", truncated)
        .replace("{existing_areas}", &areas_block)
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

pub fn spawn_extraction_worker(
    svc: &Arc<ServiceContext>,
    mem_item_id: String,
    title: String,
    body: String,
    kind: MemKind,
) -> Option<String> {
    if !should_extract(kind) || body.len() < MIN_BODY_LEN {
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
