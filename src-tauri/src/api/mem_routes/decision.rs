//! Stage-2 decision re-rank over mem recall, plus its bearer-only token and status routes.

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::{
    extract::{Query, State as AxumState},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::search::{
    baseline_search, mem_hit_value, mem_query, RecallBackend, SearchError, SearchParams,
};
use crate::api::routes::AppState;
use crate::config::schema::{MemoryRerankConfig, SupporterConfig};
use crate::mem::decision::rerank::{
    run_stage, run_stage_until, FallbackReason, RerankCandidate, StageConfig, StageOutcome,
};
use crate::mem::decision::token_store::{TokenStatus, TokenStore};
use crate::mem::decision::typesafe::TypeSafeAdapter;
use crate::mem::decision::{DecisionModel, DecisionModelInfo};
use crate::mem::rerank::RerankConfig;
use crate::mem::search::{SearchConfig, SearchQuery};
use crate::mem::SearchHit;
use crate::service::plan::ports::PlanIndexQuery;
use crate::service::plan::search_index::PlanSearchHit;
use crate::service::ServiceContext;

pub(crate) mod sessions_source;

pub type SupporterRegistry = codeterm_plugin::capability::CapabilityRegistry<dyn DecisionModel>;

/// Mirrors the `codeterm-mem` recall default for a request that names no limit.
const DEFAULT_RECALL_LIMIT: usize = 5;
const MAX_POOL: usize = 50;
const ENVELOPE_KEY: &str = "supporter";

pub(crate) fn supporter_token_dir() -> PathBuf {
    codeterm_paths::data_dir().join("supporter")
}

fn registered_adapters(models: &SupporterRegistry) -> Vec<Value> {
    let mut ids = models.ids();
    ids.sort();
    ids.into_iter()
        .filter_map(|id| {
            let model = models.get(&id)?;
            let primitives = model.primitives();
            Some(json!({
                "id": id,
                "primitives": {
                    "choice": primitives.choice,
                    "noul": primitives.noul,
                    "score": primitives.score,
                },
                "models": model.models(),
            }))
        })
        .collect()
}

fn supporter_state(
    features_on: bool,
    token: &TokenStatus,
    adapter_registered: bool,
) -> &'static str {
    if !token.present {
        return if features_on {
            "feature_without_token"
        } else {
            "no_token"
        };
    }
    if token.mode_ok == Some(false) {
        return "token_unusable";
    }
    if !adapter_registered {
        return "adapter_unavailable";
    }
    if features_on {
        "active"
    } else {
        "token_only"
    }
}

fn select_model_id(
    adapter: &str,
    available: &[DecisionModelInfo],
    requested: Option<String>,
    current_adapter: bool,
    current_model: Option<String>,
) -> Result<Option<String>, String> {
    match requested {
        Some(id) if available.iter().any(|model| model.id == id) => Ok(Some(id)),
        Some(id) => {
            let listed = available
                .iter()
                .map(|model| model.id.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            Err(format!(
                "model {id} is not listed by adapter {adapter}; listed models: {listed}"
            ))
        }
        None if current_adapter
            && current_model
                .as_ref()
                .is_some_and(|id| available.iter().any(|model| &model.id == id)) =>
        {
            Ok(current_model)
        }
        None => Ok(available.first().map(|model| model.id.clone())),
    }
}

fn token_store() -> TokenStore {
    TokenStore::new(supporter_token_dir())
}

pub(crate) fn default_registry() -> Arc<SupporterRegistry> {
    let cfg = crate::config::load_config().supporter;
    let registry = SupporterRegistry::new();
    registry.register(
        TypeSafeAdapter::ID,
        Arc::new(TypeSafeAdapter::new(token_store(), cfg.endpoint, cfg.model)),
    );
    Arc::new(registry)
}

/// Recall for the decision path: pool reads record no access; only returned mem hits do, once.
pub(crate) trait DecisionBackend: RecallBackend {
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

impl DecisionBackend for ServiceContext {
    fn recall_untouched(
        &self,
        q: &SearchQuery,
        cfg: &SearchConfig,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.recall_with_config_untouched(q, cfg)
    }

    fn legacy_rerank_untouched(
        &self,
        q: &SearchQuery,
        cfg: &RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.search_with_rerank_untouched(q, cfg)
    }

    fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String> {
        self.mem.graph_candidates_untouched(q, m)
    }

    fn session_candidates(
        &self,
        query: &str,
        limit: usize,
        max_body_chars: usize,
        deadline: Instant,
    ) -> Vec<sessions_source::SessionCandidate> {
        sessions_source::search(
            self.agent_registry.clone(),
            query,
            limit,
            max_body_chars,
            deadline,
        )
    }

    fn record_access(&self, ids: &[String]) -> Result<(), String> {
        self.mem.record_access(ids)
    }
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

pub(crate) fn skipped_search<B: DecisionBackend + ?Sized>(
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
pub(crate) fn decision_search<B: DecisionBackend + ?Sized>(
    backend: &B,
    models: &SupporterRegistry,
    supporter: &SupporterConfig,
    p: &SearchParams,
) -> Result<Value, SearchError> {
    let cfg = &supporter.memory_rerank;
    let Some(model) = models.get(&supporter.adapter) else {
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
    let docs = if docs_on {
        crate::mem::docs_source::search_with_hybrid(&p.q, cfg.sources.docs, cfg.docs_hybrid)
    } else {
        Vec::new()
    };
    let docs_count = docs.len();
    for doc in docs {
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
        hits.push(session.hit);
        candidates.push(session.candidate);
    }
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
                            object.insert("via".into(), json!(via));
                        }
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

fn doc_hit_value(doc: &crate::mem::docs_source::DocCandidate) -> Value {
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

fn doc_candidate(doc: &crate::mem::docs_source::DocCandidate) -> RerankCandidate {
    RerankCandidate {
        key: format!("doc:{}#{}", doc.doc_name, doc.anchor),
        title: format!("{} § {}", doc.doc_name, doc.section_title),
        kind: "doc".into(),
        body: doc.text.clone(),
    }
}

#[derive(Deserialize)]
pub struct TokenSetBody {
    adapter: Option<String>,
    token: String,
}

#[derive(Deserialize)]
pub struct AdapterQuery {
    adapter: Option<String>,
}

#[derive(Deserialize)]
pub struct SupporterUseBody {
    adapter: String,
    model: Option<String>,
}

#[derive(Deserialize)]
pub struct SupporterFeaturesBody {
    memory_rerank: Option<bool>,
    tab_state: Option<bool>,
}

type RouteResult = Result<Json<Value>, (StatusCode, String)>;

pub async fn supporter_token_set(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<TokenSetBody>,
) -> RouteResult {
    let adapter = body
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    if !state.svc.supporter_models.contains(&adapter) {
        return Err(unknown_adapter(&state.svc.supporter_models, &adapter));
    }
    let token = body.token;
    let status = token_io(move || {
        let store = token_store();
        store.set(&adapter, &token)?;
        store.status(&adapter).map(|status| (adapter, status))
    })
    .await?;
    Ok(Json(token_status_json(&status.0, &status.1)))
}

pub async fn supporter_token_clear(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let adapter = query
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    let (adapter, removed) = token_io(move || {
        let removed = token_store().clear(&adapter)?;
        Ok((adapter, removed))
    })
    .await?;
    Ok(Json(json!({ "adapter": adapter, "removed": removed })))
}

pub async fn supporter_token_status(
    AxumState(state): AxumState<AppState>,
    Query(query): Query<AdapterQuery>,
) -> RouteResult {
    let adapter = query
        .adapter
        .unwrap_or_else(|| state.config.lock().supporter.adapter.clone());
    let (adapter, status) = token_io(move || {
        let status = token_store().status(&adapter)?;
        Ok((adapter, status))
    })
    .await?;
    Ok(Json(token_status_json(&adapter, &status)))
}

pub async fn supporter_status(AxumState(state): AxumState<AppState>) -> RouteResult {
    let models = state.svc.supporter_models.clone();
    let cfg = state.config.lock().supporter.clone();
    token_io(move || {
        let store = token_store();
        let mut adapters = registered_adapters(&models);
        for adapter in &mut adapters {
            if let Some(id) = adapter.get("id").and_then(Value::as_str) {
                let status = store.status(id)?;
                adapter["token"] = token_status_json(id, &status);
            }
        }
        let selected_token = store.status(&cfg.adapter)?;
        let features_on = cfg.memory_rerank.enabled || cfg.tab_state.enabled;
        let adapter_registered = models.contains(&cfg.adapter);
        let state = supporter_state(features_on, &selected_token, adapter_registered);
        let model = cfg.model.clone().or_else(|| {
            models.get(&cfg.adapter).and_then(|model| {
                model
                    .models()
                    .first()
                    .map(|entry| entry.id.clone())
            })
        });
        Ok(json!({
            "state": state,
            "adapter": cfg.adapter,
            "model": model,
            "deadline_ms": cfg.deadline_ms,
            "features": {
                "memory_rerank": cfg.memory_rerank.enabled,
                "tab_state": cfg.tab_state.enabled,
            },
            "token": token_status_json(&cfg.adapter, &selected_token),
            "adapter_registered": adapter_registered,
            "adapters": adapters,
        }))
    })
    .await
    .map(Json)
}

pub async fn supporter_models(AxumState(state): AxumState<AppState>) -> RouteResult {
    let cfg = state.config.lock().supporter.clone();
    let models = state.svc.supporter_models.clone();
    let adapters = registered_adapters(&models);
    Ok(Json(json!({
        "adapter": cfg.adapter,
        "model": cfg.model,
        "adapters": adapters,
    })))
}

pub async fn supporter_use(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<SupporterUseBody>,
) -> RouteResult {
    let Some(adapter) = state.svc.supporter_models.get(&body.adapter) else {
        return Err(unknown_adapter(&state.svc.supporter_models, &body.adapter));
    };
    let available = adapter.models();
    let current = state.config.lock().supporter.clone();
    let selected_model = select_model_id(
        &body.adapter,
        &available,
        body.model,
        current.adapter == body.adapter,
        current.model,
    )
    .map_err(|error| (StatusCode::BAD_REQUEST, error))?;
    if let Some(model_id) = selected_model.as_deref() {
        if !adapter.select_model(model_id) {
            return Err((
                StatusCode::BAD_REQUEST,
                format!("adapter {} cannot select model {model_id}", body.adapter),
            ));
        }
    }
    let adapter_id = body.adapter;
    let cfg = update_supporter_config(state, move |cfg| {
        cfg.adapter = adapter_id;
        cfg.model = selected_model;
    })
    .await?;
    Ok(Json(json!({ "adapter": cfg.adapter, "model": cfg.model })))
}

pub async fn supporter_features(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<SupporterFeaturesBody>,
) -> RouteResult {
    if body.memory_rerank.is_none() && body.tab_state.is_none() {
        return Err((
            StatusCode::BAD_REQUEST,
            "set memory_rerank or tab_state".into(),
        ));
    }
    if body.memory_rerank == Some(true) || body.tab_state == Some(true) {
        let adapter = state.config.lock().supporter.adapter.clone();
        let token_adapter = adapter.clone();
        let status = token_io(move || token_store().status(&token_adapter)).await?;
        if !status.present || status.mode_ok == Some(false) {
            return Err((
                StatusCode::CONFLICT,
                format!("no usable supporter token is stored for adapter {adapter}"),
            ));
        }
    }
    let cfg = update_supporter_config(state, move |cfg| {
        if let Some(enabled) = body.memory_rerank {
            cfg.memory_rerank.enabled = enabled;
        }
        if let Some(enabled) = body.tab_state {
            cfg.tab_state.enabled = enabled;
        }
    })
    .await?;
    Ok(Json(json!({
        "memory_rerank": cfg.memory_rerank.enabled,
        "tab_state": cfg.tab_state.enabled,
    })))
}

fn token_status_json(adapter: &str, status: &TokenStatus) -> Value {
    json!({
        "adapter": adapter,
        "present": status.present,
        "path": status.path,
        "mode_ok": status.mode_ok,
    })
}

fn unknown_adapter(models: &SupporterRegistry, adapter: &str) -> (StatusCode, String) {
    let mut registered = models.ids();
    registered.sort();
    (
        StatusCode::NOT_FOUND,
        format!(
            "supporter adapter is not registered: {adapter}; registered adapters: {}",
            if registered.is_empty() {
                "none".to_owned()
            } else {
                registered.join(", ")
            }
        ),
    )
}

async fn update_supporter_config<F>(
    state: AppState,
    update: F,
) -> Result<crate::config::schema::SupporterConfig, (StatusCode, String)>
where
    F: FnOnce(&mut crate::config::schema::SupporterConfig) + Send + 'static,
{
    let config_state = state.config.clone();
    tokio::task::spawn_blocking(move || {
        let mut config = config_state.lock().clone();
        update(&mut config.supporter);
        crate::config::bounds::validate_app_config(&config)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidInput, error))?;
        let supporter = config.supporter.clone();
        crate::config::save_config_and_apply(&config_state, config)
            .map_err(|error| io::Error::new(io::ErrorKind::Other, error))?;
        Ok::<_, io::Error>(supporter)
    })
    .await
    .map_err(|error| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("supporter config task failed: {error}"),
        )
    })?
    .map_err(|error| {
        let status = if error.kind() == io::ErrorKind::InvalidInput {
            StatusCode::BAD_REQUEST
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };
        (status, format!("supporter config update failed: {error}"))
    })
}

/// Token file IO stays off the async runtime; io errors never carry the token value.
async fn token_io<T, F>(work: F) -> Result<T, (StatusCode, String)>
where
    T: Send + 'static,
    F: FnOnce() -> io::Result<T> + Send + 'static,
{
    tokio::task::spawn_blocking(work)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("supporter token task failed: {error}"),
            )
        })?
        .map_err(|error| {
            let status = if error.kind() == io::ErrorKind::InvalidInput {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, format!("supporter token store: {error}"))
        })
}

#[cfg(test)]
mod tests;
