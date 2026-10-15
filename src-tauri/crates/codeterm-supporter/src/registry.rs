//! Stage-2 decision re-rank over mem recall.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_decision::{DecisionModel, TokenMode};
use serde_json::{json, Value};

use crate::search::{
    baseline_search, mem_hit_value, mem_query, RecallBackend, SearchError, SearchParams,
};
use codeterm_mem::decision::rerank::{
    run_stage, run_stage_until, FallbackReason, RerankCandidate, StageConfig, StageOutcome,
};
use codeterm_mem::decision::token_store::{TokenStatus, TokenStore};
use codeterm_mem::decision::typesafe::TypeSafeAdapter;
use codeterm_mem::rerank::RerankConfig;
use codeterm_mem::search::{SearchConfig, SearchQuery};
use codeterm_mem::SearchHit;
use codeterm_plan::service_plan::ports::PlanIndexQuery;
use codeterm_plan::service_plan::search_index::PlanSearchHit;
use codeterm_types::config::{MemoryRerankConfig, SupporterConfig};

pub mod sessions_source;

pub type SupporterRegistry = codeterm_plugin::capability::CapabilityRegistry<dyn DecisionModel>;

/// Mirrors the `codeterm-mem` recall default for a request that names no limit.
const DEFAULT_RECALL_LIMIT: usize = 5;
const MAX_POOL: usize = 50;
const ENVELOPE_KEY: &str = "supporter";

pub fn supporter_token_dir() -> PathBuf {
    codeterm_paths::data_dir().join("supporter")
}

pub fn registered_adapters(models: &SupporterRegistry) -> Vec<Value> {
    let mut ids = models.ids();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let model = models.get(&id)?;
            let primitives = model.primitives();
            Some(json!({
                "id": id,
                "token_mode": token_mode_name(model.token_mode()),
                "primitives": {
                    "choice": primitives.choice,
                    "noul": primitives.noul,
                    "score": primitives.score,
                },
                "models": model.models(),
                "pricing": model.pricing(),
                "metadata": model.metadata(),
            }))
        })
        .collect()
}

pub fn supporter_state(
    selected: bool,
    token: &TokenStatus,
    adapter_registered: bool,
    token_mode: TokenMode,
) -> &'static str {
    if !selected {
        return "not_connected";
    }
    if !adapter_registered {
        return "adapter_unavailable";
    }
    if token_mode == TokenMode::Required {
        if !token.present {
            return "no_token";
        }
        if token.mode_ok == Some(false) {
            return "token_unusable";
        }
    }
    "active"
}

pub fn token_mode_name(mode: TokenMode) -> &'static str {
    match mode {
        TokenMode::Required => "required",
        TokenMode::Optional => "optional",
        TokenMode::None => "none",
    }
}

pub fn token_store() -> TokenStore {
    TokenStore::new(supporter_token_dir())
}

pub fn default_registry() -> Arc<SupporterRegistry> {
    let cfg = codeterm_config::load_config().supporter;
    let registry = SupporterRegistry::with_registration_hook(|id, adapter| {
        crate::restore_model(&codeterm_config::load_config().supporter, id, adapter);
    });
    registry.register(
        TypeSafeAdapter::ID,
        Arc::new(TypeSafeAdapter::new(
            token_store(),
            cfg.connection(TypeSafeAdapter::ID).endpoint,
            cfg.connection(TypeSafeAdapter::ID).model,
        )),
    );
    Arc::new(registry)
}

/// Recall for the decision path: pool reads record no access; only returned mem hits do, once.
pub trait DecisionBackend: RecallBackend {
    fn supporter_runtime(&self) -> Arc<crate::SupporterRuntime> {
        crate::runtime()
    }
    fn recall_untouched(
        &self,
        q: &SearchQuery,
        cfg: &SearchConfig,
    ) -> Result<Vec<SearchHit>, String>;
    fn legacy_rerank_untouched(
        &self,
        q: &SearchQuery,
        cfg: &RerankConfig,
    ) -> Result<Vec<SearchHit>, String>;
    fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String>;
    fn session_candidates(
        &self,
        _: &str,
        _: usize,
        _: usize,
        _: Instant,
    ) -> Vec<sessions_source::SessionCandidate> {
        Vec::new()
    }
    fn record_access(&self, ids: &[String]) -> Result<(), String>;
}

/// Runs the unchanged baseline over access-free reads.
struct Untouched<'a, B: ?Sized>(&'a B);

impl<B: DecisionBackend + ?Sized> RecallBackend for Untouched<'_, B> {
    fn recall(&self, q: &SearchQuery, cfg: &SearchConfig) -> Result<Vec<SearchHit>, String> {
        self.0.recall_untouched(q, cfg)
    }

    fn legacy_rerank(&self, q: &SearchQuery, cfg: &RerankConfig) -> Result<Vec<SearchHit>, String> {
        self.0.legacy_rerank_untouched(q, cfg)
    }

    fn plan_hits(&self, project: &str, q: &PlanIndexQuery) -> Result<Vec<PlanSearchHit>, String> {
        self.0.plan_hits(project, q)
    }
}

/// The baseline response for `p`, recording access exactly for the mem hits it returns.
fn served_baseline<B: DecisionBackend + ?Sized>(
    backend: &B,
    p: &SearchParams,
) -> Result<Value, SearchError> {
    let output = baseline_search(&Untouched(backend), p)?;
    record_returned(backend, &output);
    Ok(output)
}

/// Like hydration's own touch, a failed access record never fails the search.
fn record_returned<B: DecisionBackend + ?Sized>(backend: &B, output: &Value) {
    let ids: Vec<String> = output["hits"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|hit| hit.get("source").and_then(Value::as_str) == Some("mem"))
        .filter_map(|hit| hit.pointer("/item/id").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    if ids.is_empty() {
        return;
    }
    if let Err(error) = backend.record_access(&ids) {
        log::warn!("[mem.supporter] access record failed: {error}");
    }
}

fn insert_envelope(output: &mut Value, envelope: Value) {
    if let Some(object) = output.as_object_mut() {
        object.insert(ENVELOPE_KEY.into(), envelope);
    }
}

pub fn skipped_search<B: DecisionBackend + ?Sized>(
    backend: &B,
    p: &SearchParams,
) -> Result<Value, SearchError> {
    let mut output = served_baseline(backend, p)?;
    insert_envelope(
        &mut output,
        json!({ "status": "skipped", "reason": "legacy_rerank" }),
    );
    Ok(output)
}

/// Every mem lock is released when `baseline_search` returns, so stage 2 runs lock-free.
pub fn decision_search<B: DecisionBackend + ?Sized>(
    backend: &B,
    models: &SupporterRegistry,
    supporter: &SupporterConfig,
    p: &SearchParams,
) -> Result<Value, SearchError> {
    let cfg = &supporter.memory_rerank;
    let Some(model) = crate::configured_model(supporter, models) else {
        return fallback(backend, p, FallbackReason::NoAdapter);
    };
    let limit = p.limit.unwrap_or(DEFAULT_RECALL_LIMIT);
    let pooled = SearchParams {
        limit: Some(
            limit
                .max(cfg.sources.recall)
                .min(cfg.candidates)
                .min(MAX_POOL),
        ),
        ..p.clone()
    };
    let mut output = baseline_search(&Untouched(backend), &pooled)?;
    let mut hits = match output.get_mut("hits").and_then(Value::as_array_mut) {
        Some(hits) => std::mem::take(hits),
        None => Vec::new(),
    };
    let graph_on = cfg.sources.graph > 0;
    let docs_on = cfg.sources.docs > 0;
    let sessions_on = cfg.sources.sessions > 0;
    let recall_count = hits.len();
    if graph_on {
        let additions = graph_additions(backend, p, &hits, cfg.sources.graph);
        hits.extend(additions);
    }
    let graph_count = hits.len() - recall_count;
    let mut candidates: Vec<RerankCandidate> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| candidate(index, hit))
        .collect();
    let mut scores: Vec<_> = hits
        .iter()
        .map(|hit| hit["score"].as_f64().unwrap_or(0.0))
        .collect();
    let docs = if docs_on {
        codeterm_mem::docs_source::search_with_hybrid(&p.q, cfg.sources.docs, cfg.docs_hybrid)
    } else {
        Vec::new()
    };
    let docs_count = docs.len();
    for doc in docs {
        scores.push(-doc.relevance);
        hits.push(doc_hit_value(&doc));
        candidates.push(doc_candidate(&doc));
    }
    let stage_config = stage_config(cfg, supporter.deadline_ms);
    let stage_deadline = sessions_on.then(|| Instant::now() + stage_config.deadline);
    let sessions = if let Some(deadline) = stage_deadline {
        let source_budget = Duration::from_millis((supporter.deadline_ms / 4).min(250));
        backend.session_candidates(
            &p.q,
            cfg.sources.sessions,
            cfg.max_body_chars,
            (Instant::now() + source_budget).min(deadline),
        )
    } else {
        Vec::new()
    };
    let sessions_count = sessions.len();
    for session in sessions {
        scores.push(session.hit["score"].as_f64().unwrap_or(0.0));
        hits.push(session.hit);
        candidates.push(session.candidate);
    }
    let skip = crate::rerank::skip_reason(&scores);
    let selected = crate::rerank::selection(&scores);
    let mut selected_hits = Vec::new();
    let mut selected_candidates = Vec::new();
    for index in selected {
        let mut hit = hits[index].clone();
        if graph_on || docs_on || sessions_on {
            let via = if index < recall_count {
                "recall"
            } else if index < recall_count + graph_count {
                "graph"
            } else if index < recall_count + graph_count + docs_count {
                "docs"
            } else {
                "sessions"
            };
            hit["via"] = json!(via);
        }
        selected_hits.push(hit);
        selected_candidates.push(candidates[index].clone());
    }
    hits = selected_hits;
    candidates = selected_candidates;
    if let Some(reason) = skip {
        if graph_count == 0 && docs_count == 0 && sessions_count == 0 {
            output = served_baseline(backend, p)?;
        } else {
            hits.truncate(limit);
            output["hits"] = json!(hits);
            record_returned(backend, &output);
        }
        insert_envelope(
            &mut output,
            json!({ "status": "skipped", "reason": reason,
            "sources": { "recall": recall_count, "graph": graph_count, "docs": docs_count, "sessions": sessions_count } }),
        );
        return Ok(output);
    }
    if !model.supports_question_batch() {
        return fallback(backend, p, FallbackReason::Unsupported);
    }
    let model = crate::budgeted_model_with_activity(
        model,
        backend.supporter_runtime(),
        supporter.calls_per_hour.unwrap_or(0),
        crate::VerdictKind::Rerank,
        p.caller_tab_id.clone(),
        None,
        Some(crate::ActivityContext::Rerank {
            query: p.q.clone(),
            titles: candidates
                .iter()
                .map(|candidate| candidate.title.clone())
                .collect(),
        }),
    );
    let outcome = match stage_deadline {
        Some(deadline) => run_stage_until(model, &p.q, &candidates, &stage_config, deadline),
        None => run_stage(model, &p.q, &candidates, &stage_config),
    };
    match outcome {
        StageOutcome::Applied {
            order,
            relevance,
            low_confidence,
            max_relevance,
        } => {
            let mut slots: Vec<Option<Value>> = hits.into_iter().map(Some).collect();
            let ranked: Vec<Value> = order
                .iter()
                .zip(relevance)
                .filter_map(|(&index, score)| {
                    let mut hit = slots.get_mut(index)?.take()?;
                    if let Some(object) = hit.as_object_mut() {
                        object.insert("relevance".into(), json!(score));
                    }
                    Some(hit)
                })
                .take(limit)
                .collect();
            output["hits"] = Value::Array(ranked);
            record_returned(backend, &output);
            let mut envelope = json!({
                "status": "applied",
                "adapter": supporter.adapter,
                "low_confidence": low_confidence,
                "max_relevance": max_relevance,
            });
            if sessions_on {
                envelope["sources"] = json!({
                    "recall": recall_count,
                    "graph": graph_count,
                    "docs": docs_count,
                    "sessions": sessions_count,
                });
            } else if docs_on {
                envelope["sources"] = json!({
                    "recall": recall_count,
                    "graph": graph_count,
                    "docs": docs_count,
                });
            } else if graph_on {
                envelope["sources"] = json!({ "recall": recall_count, "graph": graph_count });
            }
            insert_envelope(&mut output, envelope);
            Ok(output)
        }
        StageOutcome::Fallback { reason } => fallback(backend, p, reason),
    }
}

/// At most `m` graph-linked mem items the recall pool lacks, shaped exactly like recall mem hits.
/// Over-fetching by the pool size keeps dedup from starving the graph slots.
fn graph_additions<B: DecisionBackend + ?Sized>(
    backend: &B,
    p: &SearchParams,
    pool: &[Value],
    m: usize,
) -> Vec<Value> {
    let pooled: HashSet<&str> = pool
        .iter()
        .filter(|hit| hit.get("source").and_then(Value::as_str) == Some("mem"))
        .filter_map(|hit| hit.pointer("/item/id").and_then(Value::as_str))
        .collect();
    let q = SearchQuery {
        limit: None,
        ..mem_query(p)
    };
    match backend.graph_candidates_untouched(&q, m + pooled.len()) {
        Ok(hits) => hits
            .into_iter()
            .filter(|hit| !pooled.contains(hit.item.id.as_str()))
            .take(m)
            .map(mem_hit_value)
            .collect(),
        Err(error) => {
            log::warn!("[mem.supporter] graph source failed: {error}");
            Vec::new()
        }
    }
}

/// Pool-size recall is not prefix-identical to limit-size recall, so fallback re-runs the user's query.
fn fallback<B: DecisionBackend + ?Sized>(
    backend: &B,
    p: &SearchParams,
    reason: FallbackReason,
) -> Result<Value, SearchError> {
    log::info!("[mem.decision] rerank fallback reason={}", reason.as_str());
    let mut output = served_baseline(backend, p)?;
    insert_envelope(
        &mut output,
        json!({ "status": "fallback", "reason": reason.as_str() }),
    );
    Ok(output)
}

fn stage_config(cfg: &MemoryRerankConfig, deadline_ms: u64) -> StageConfig {
    StageConfig {
        request_mode: codeterm_types::config::RerankRequestMode::SharedState,
        max_concurrency: cfg.max_concurrency,
        deadline: Duration::from_millis(deadline_ms),
        low_confidence_threshold: cfg.low_confidence_threshold,
        max_body_chars: cfg.max_body_chars,
    }
}

/// Mem hits nest the item under `item`; plan hits carry `title`/`body` at the top level.
fn candidate(index: usize, hit: &Value) -> RerankCandidate {
    let fields = hit.get("item").unwrap_or(hit);
    let text_of = |key: &str| fields.get(key).and_then(Value::as_str).unwrap_or("");
    let key = match fields.get("id").and_then(Value::as_str) {
        Some(id) => id.to_string(),
        None => index.to_string(),
    };
    RerankCandidate {
        key,
        title: text_of("title").to_string(),
        kind: text_of("kind").to_string(),
        body: text_of("body").to_string(),
    }
}

fn doc_hit_value(doc: &codeterm_mem::docs_source::DocCandidate) -> Value {
    let pointer = if doc.doc_name == "CLI" {
        format!("{} --help", doc.section_title)
    } else {
        format!("codeterm docs {}", doc.doc_name)
    };
    json!({
        "source": "doc",
        "doc": doc.doc_name,
        "section": doc.section_title,
        "anchor": doc.anchor,
        "pointer": pointer,
    })
}

fn doc_candidate(doc: &codeterm_mem::docs_source::DocCandidate) -> RerankCandidate {
    RerankCandidate {
        key: format!("doc:{}#{}", doc.doc_name, doc.anchor),
        title: format!("{} § {}", doc.doc_name, doc.section_title),
        kind: "doc".into(),
        body: doc.text.clone(),
    }
}

#[cfg(test)]
mod tests;
