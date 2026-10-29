//! Hidden graph-worker that extracts entities/relations from mem_items.
//! Also hosts the per-turn evolution observer scaffold (Track 8 of v1.1.23,
//! issue #58) — bumps `evidence_count` on skill/lesson mem_items when the
//! next observation is success-shaped. Auto-retire / rank formula land in
//! v1.1.24; this is foundation only.

use std::collections::HashMap;

use parking_lot::Mutex;

use codeterm_types::config::MemoryConfig;

use crate::ports::MemHostContext;
use crate::store::MemKind;
use crate::MemManager;

fn should_extract(kind: MemKind) -> bool {
    !matches!(kind, MemKind::Summary)
}

pub fn should_extract_public(kind: MemKind) -> bool {
    should_extract(kind)
}

pub fn build_extraction_prompt(
    host: &dyn MemHostContext,
    mem_cfg: &MemoryConfig,
    mem_item_id: &str,
    title: &str,
    body: &str,
) -> String {
    let truncated = if body.len() > 50_000 {
        let mut end = 50_000;
        while !body.is_char_boundary(end) {
            end -= 1;
        }
        &body[..end]
    } else {
        body
    };
    let areas_block = format_existing_areas(host);
    let graph_types_block = format_existing_graph_types(host);
    let dedup_threshold = mem_cfg.dedup_threshold;
    crate::prompt_text::MEM_EXTRACTION
        .replace("{id}", mem_item_id)
        .replace("{title}", title)
        .replace("{body}", truncated)
        .replace("{existing_areas}", &areas_block)
        .replace("{existing_graph_types}", &graph_types_block)
        .replace("{dedup_threshold}", &format!("{:.2}", dedup_threshold))
}

// Feed existing taxonomy into prompts so workers pick known mem areas by usage order.
// Prompt only suggests existing areas; only main agent can create new ones.
fn format_existing_areas(host: &dyn MemHostContext) -> String {
    let tax = match host.get_taxonomy(None) {
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

fn format_existing_graph_types(host: &dyn MemHostContext) -> String {
    let (entity_types, relation_types) = host.query_graph_type_counts();

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

/// Find mem_items that are eligible for extraction but have no child items
/// linked back via `extracted_from`. These are either (a) never extracted,
/// or (b) extractions that produced zero child items. Caller decides.
pub fn find_pending_items(
    mem: &MemManager,
    limit: usize,
) -> Result<Vec<(String, String, String, MemKind)>, String> {
    mem.store.pending_extraction(limit)
}

// Track injected `kind=skill` and `kind=lesson` items so we can bump evidence on matching output.
// Only successful-shaped observations count; raw echoes and failed/error-looking outputs are ignored.

/// Tracked context entry per scoped key (e.g. tab id, worker id).
#[derive(Clone, Debug)]
pub struct InjectedItem {
    pub mem_id: String,
    pub kind: MemKind,
    /// First ~512 chars of the body, lowercased. Used to detect when a
    /// later observation is just quoting the skill back at us.
    pub body_signature: String,
}

impl InjectedItem {
    pub fn new(mem_id: String, kind: MemKind, body: &str) -> Self {
        let mut sig: String = body.chars().take(512).collect();
        sig.make_ascii_lowercase();
        Self {
            mem_id,
            kind,
            body_signature: sig,
        }
    }
}

/// In-memory ring of tracked injections keyed by scope (tab / worker).
/// One entry per scope holds the most-recent batch of skill/lesson ids.
/// Cleared when an observation is consumed against it.
pub struct EvolutionObserver {
    by_scope: Mutex<HashMap<String, Vec<InjectedItem>>>,
}

impl Default for EvolutionObserver {
    fn default() -> Self {
        Self {
            by_scope: Mutex::new(HashMap::new()),
        }
    }
}

impl EvolutionObserver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record that `items` were injected into `scope`'s context. Replaces any
    /// previous batch for the scope — only the most-recent set is in flight.
    pub fn record_injection(&self, scope: &str, items: Vec<InjectedItem>) {
        let mut by_scope = self.by_scope.lock();
        if items.is_empty() {
            by_scope.remove(scope);
        } else {
            by_scope.insert(scope.to_string(), items);
        }
    }

    /// Drain the tracked items for `scope` and apply the observation.
    /// Returns the list of mem_ids that got `evidence_count` bumped.
    /// `observation` is the raw text of the next agent output / tool result.
    pub fn observe(
        &self,
        store: &dyn crate::backend::MemStore,
        scope: &str,
        observation: &str,
    ) -> Vec<String> {
        self.observe_evidence(scope, observation, |id| {
            store.bump_evidence(
                &crate::backend::WriteContext::system("evolution-observer"),
                id,
            )
        })
    }
    pub(crate) fn observe_evidence(
        &self,
        scope: &str,
        observation: &str,
        bump: impl Fn(&str) -> Result<bool, String>,
    ) -> Vec<String> {
        let items = {
            let mut by_scope = self.by_scope.lock();
            by_scope.remove(scope).unwrap_or_default()
        };
        if items.is_empty() {
            return Vec::new();
        }
        if !is_success_shaped(observation) {
            return Vec::new();
        }
        let obs_lower = observation.to_ascii_lowercase();
        let mut bumped = Vec::new();
        for item in items {
            if observation_echoes_body(&obs_lower, &item.body_signature) {
                continue;
            }
            match bump(&item.mem_id) {
                Ok(true) => bumped.push(item.mem_id),
                Ok(false) => {
                    log::debug!(
                        "[evolution-observer] mem_id {} no longer present, skipping bump",
                        item.mem_id
                    );
                }
                Err(e) => {
                    log::warn!(
                        "[evolution-observer] bump failed for {}: {}",
                        item.mem_id,
                        e
                    );
                }
            }
        }
        bumped
    }

    #[cfg(test)]
    fn pending(&self, scope: &str) -> Vec<InjectedItem> {
        self.by_scope.lock().get(scope).cloned().unwrap_or_default()
    }
}

/// Crude success heuristic for v1.1.23: no explicit error markers.
/// v1.1.24 will tighten this with exit-code awareness + corrective-tool
/// detection.
fn is_success_shaped(observation: &str) -> bool {
    let lower = observation.to_ascii_lowercase();
    !(lower.contains("error")
        || lower.contains("failed")
        || lower.contains("panic")
        || lower.contains("traceback"))
}

/// False-corroboration mitigation: if the observation is largely a quote
/// of the injected body it doesn't count as independent evidence. We
/// require at least 80 contiguous chars of overlap from the signature.
fn observation_echoes_body(observation_lower: &str, body_sig_lower: &str) -> bool {
    if body_sig_lower.len() < 80 {
        return false;
    }
    let probe_len = body_sig_lower.len().min(120);
    let probe = &body_sig_lower[..probe_len];
    observation_lower.contains(probe)
}

#[cfg(test)]
mod tests;
