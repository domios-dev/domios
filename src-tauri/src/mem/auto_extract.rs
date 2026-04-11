//! Auto-extraction: on save, spawn a hidden graph-worker that uses the
//! configured agent provider + model to extract entities and relations
//! from text, then writes them back via `codeterm mem` CLI commands.
//!
//! Free when the default provider is Claude Max (OAuth, no API key).
//! Workers show up in the Memory Panel "Workers" tab as a log of what
//! was extracted. The pane is hidden from birth — zero tab flash.
//!
//! Phase 3 step 2 of the spawn unification: this module no longer owns
//! any pane/PTY/reader plumbing. It builds the extraction prompt and
//! delegates the whole spawn through `ServiceContext::spawn_pane`.

use std::sync::Arc;

use crate::mem::store::MemKind;
use crate::service::spawn::{
    AgentSpawnSpec, PaneCommand, PaneVisibility, SpawnPaneSpec,
};
use crate::service::ServiceContext;

/// Kinds that trigger auto-extraction on save.
/// Facts and notes contain rich content worth extracting entities from.
/// Preferences and rules are short, structured — skip extraction.
/// Skills have their own indexing path.
fn should_extract(kind: MemKind) -> bool {
    matches!(
        kind,
        MemKind::Conversation | MemKind::Note | MemKind::Daily | MemKind::Learning | MemKind::Fact
    )
}

/// Minimum body length to justify spawning a worker.
const MIN_BODY_LEN: usize = 80;

fn build_extraction_prompt(mem_item_id: &str, title: &str, body: &str) -> String {
    let truncated = if body.len() > 4000 { &body[..4000] } else { body };
    crate::prompts::MEM_EXTRACTION
        .replace("{id}", mem_item_id)
        .replace("{title}", title)
        .replace("{body}", truncated)
}

/// Spawn a hidden extraction worker for a freshly-saved mem_item.
/// Returns the worker id on success (so callers can track completion via
/// `svc.workers.get(worker_id)`), or `None` if the item doesn't qualify
/// (short body, non-extractable kind) or the spawn failed.
///
/// The concurrency cap lives in the caller: `mem_save` in mem_routes.rs
/// holds an `extract_semaphore` permit across this call + a polling loop
/// that waits for the worker to reach a terminal state.
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

    let prompt = build_extraction_prompt(&mem_item_id, &title, &body);

    let mem_cfg = crate::config::memory_config();
    let provider = if mem_cfg.extraction_agent.is_empty() {
        crate::config::default_agent_provider()
    } else {
        mem_cfg.extraction_agent.clone()
    };
    let model = if mem_cfg.extraction_model.is_empty() {
        None
    } else {
        Some(mem_cfg.extraction_model.clone())
    };

    let spec = SpawnPaneSpec {
        visibility: PaneVisibility::Hidden,
        command: PaneCommand::Agent(AgentSpawnSpec {
            provider,
            model,
            task: Some(prompt),
            auto_approve: true,
            spawned_by: Some("auto:save".into()),
            ..Default::default()
        }),
        ..Default::default()
    };

    match svc.spawn_pane(spec) {
        Ok(handle) => {
            log::info!(
                "[auto-extract] headless worker {} pane={} for {}",
                handle.worker_id.as_deref().unwrap_or("?"),
                handle.pane_id,
                mem_item_id
            );
            handle.worker_id
        }
        Err(e) => {
            log::warn!("[auto-extract] spawn failed for {}: {}", mem_item_id, e);
            None
        }
    }
}
