//! Stage-2 decision re-rank over mem recall, plus its bearer-only token and status routes.

use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{Query, State as AxumState},
    http::StatusCode,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use super::search::{baseline_search, RecallBackend, SearchError, SearchParams};
use crate::api::routes::AppState;
use crate::config::schema::DecisionRerankConfig;
use crate::mem::decision::rerank::{
    run_stage, FallbackReason, RerankCandidate, StageConfig, StageOutcome,
};
use crate::mem::decision::token_store::{TokenStatus, TokenStore};
use crate::mem::decision::typesafe::TypeSafeAdapter;
use crate::mem::decision::DecisionModel;
use crate::mem::rerank::RerankConfig;
use crate::mem::search::{SearchConfig, SearchQuery};
use crate::mem::SearchHit;
use crate::service::plan::ports::PlanIndexQuery;
use crate::service::plan::search_index::PlanSearchHit;
use crate::service::ServiceContext;

pub type DecisionRegistry = codeterm_plugin::capability::CapabilityRegistry<dyn DecisionModel>;

/// Mirrors the `codeterm-mem` recall default for a request that names no limit.
const DEFAULT_RECALL_LIMIT: usize = 5;
const MAX_POOL: usize = 50;
const ENVELOPE_KEY: &str = "decision_rerank";

pub(crate) fn decision_token_dir() -> PathBuf {
    codeterm_paths::data_dir().join("decision-models")
}

fn token_store() -> TokenStore {
    TokenStore::new(decision_token_dir())
}

pub(crate) fn default_registry() -> Arc<DecisionRegistry> {
    let cfg = crate::config::memory_config().decision_rerank;
    let registry = DecisionRegistry::new();
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
    if let Err(error) = backend.record_access(&ids) {
        log::warn!("[mem.decision] access record failed: {error}");
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
    models: &DecisionRegistry,
    cfg: &DecisionRerankConfig,
    p: &SearchParams,
) -> Result<Value, SearchError> {
    let Some(model) = models.get(&cfg.adapter) else {
        return fallback(backend, p, FallbackReason::NoAdapter);
    };
    let limit = p.limit.unwrap_or(DEFAULT_RECALL_LIMIT);
    let pooled = SearchParams {
        limit: Some(limit.max(cfg.candidates).min(MAX_POOL)),
        ..p.clone()
    };
    let mut output = baseline_search(&Untouched(backend), &pooled)?;
    let hits = match output.get_mut("hits").and_then(Value::as_array_mut) {
        Some(hits) => std::mem::take(hits),
        None => Vec::new(),
    };
    let candidates: Vec<RerankCandidate> = hits
        .iter()
        .enumerate()
        .map(|(index, hit)| candidate(index, hit))
        .collect();
    match run_stage(model, &p.q, &candidates, &stage_config(cfg)) {
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
            insert_envelope(
                &mut output,
                json!({
                    "status": "applied",
                    "adapter": cfg.adapter,
                    "low_confidence": low_confidence,
                    "max_relevance": max_relevance,
                }),
            );
            Ok(output)
        }
        StageOutcome::Fallback { reason } => fallback(backend, p, reason),
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

fn stage_config(cfg: &DecisionRerankConfig) -> StageConfig {
    StageConfig {
        max_concurrency: cfg.max_concurrency,
        deadline: Duration::from_millis(cfg.deadline_ms),
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
        text: format!("{}\n{}", text_of("title"), text_of("body")),
    }
}

#[derive(Deserialize)]
pub struct TokenSetBody {
    adapter: String,
    token: String,
}

#[derive(Deserialize)]
pub struct AdapterQuery {
    adapter: String,
}

type RouteResult = Result<Json<Value>, (StatusCode, String)>;

pub async fn decision_token_set(
    AxumState(state): AxumState<AppState>,
    Json(body): Json<TokenSetBody>,
) -> RouteResult {
    if !state.svc.decision_models.contains(&body.adapter) {
        return Err(unknown_adapter(&body.adapter));
    }
    let TokenSetBody { adapter, token } = body;
    let status = token_io(move || {
        let store = token_store();
        store.set(&adapter, &token)?;
        store.status(&adapter).map(|status| (adapter, status))
    })
    .await?;
    Ok(Json(token_status_json(&status.0, &status.1)))
}

pub async fn decision_token_clear(Query(query): Query<AdapterQuery>) -> RouteResult {
    let adapter = query.adapter;
    let (adapter, removed) = token_io(move || {
        let removed = token_store().clear(&adapter)?;
        Ok((adapter, removed))
    })
    .await?;
    Ok(Json(json!({ "adapter": adapter, "removed": removed })))
}

pub async fn decision_token_status(Query(query): Query<AdapterQuery>) -> RouteResult {
    let adapter = query.adapter;
    let (adapter, status) = token_io(move || {
        let status = token_store().status(&adapter)?;
        Ok((adapter, status))
    })
    .await?;
    Ok(Json(token_status_json(&adapter, &status)))
}

pub async fn decision_status(AxumState(state): AxumState<AppState>) -> RouteResult {
    let models = state.svc.decision_models.clone();
    token_io(move || {
        let cfg = crate::config::memory_config().decision_rerank;
        let store = token_store();
        let mut ids = models.ids();
        ids.sort();
        let mut adapters = Vec::with_capacity(ids.len());
        for id in ids {
            let Some(model) = models.get(&id) else {
                continue;
            };
            let primitives = model.primitives();
            let status = store.status(&id)?;
            adapters.push(json!({
                "id": id,
                "primitives": {
                    "choice": primitives.choice,
                    "noul": primitives.noul,
                    "score": primitives.score,
                },
                "token": token_status_json(&id, &status),
            }));
        }
        Ok(json!({
            "config": cfg,
            "adapter_registered": models.contains(&cfg.adapter),
            "adapters": adapters,
        }))
    })
    .await
    .map(Json)
}

fn token_status_json(adapter: &str, status: &TokenStatus) -> Value {
    json!({
        "adapter": adapter,
        "present": status.present,
        "path": status.path,
        "mode_ok": status.mode_ok,
    })
}

fn unknown_adapter(adapter: &str) -> (StatusCode, String) {
    (
        StatusCode::NOT_FOUND,
        format!("decision adapter is not registered: {adapter}"),
    )
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
                format!("decision token task failed: {error}"),
            )
        })?
        .map_err(|error| {
            let status = if error.kind() == io::ErrorKind::InvalidInput {
                StatusCode::BAD_REQUEST
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            (status, format!("decision token store: {error}"))
        })
}

#[cfg(test)]
mod tests;
