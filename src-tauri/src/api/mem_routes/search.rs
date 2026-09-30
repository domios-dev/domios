use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use super::decision::{self, DecisionBackend, DecisionRegistry};
use crate::api::routes::{check_auth, AppState};
use crate::config::schema::DecisionRerankConfig;
use crate::mem::{
    rerank::RerankConfig,
    search::{SearchConfig, SearchMode, SearchQuery},
    SearchHit,
};
use crate::service::plan::ports::PlanIndexQuery;
use crate::service::plan::search_index::PlanSearchHit;
use crate::service::{ServiceContext, ServiceState};

pub(crate) const RERANK_EXCLUSIVE: &str = "rerank and decision_rerank are mutually exclusive";

#[derive(Deserialize, Clone, Default)]
pub(crate) struct SearchParams {
    pub q: String,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub scope: Option<String>,
    #[serde(default)]
    pub area: Option<String>,
    #[serde(default)]
    pub limit: Option<usize>,
    #[serde(default)]
    pub rerank: Option<bool>,
    #[serde(default)]
    pub decision_rerank: Option<bool>,
    #[serde(default)]
    pub project: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub weight: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum SearchError {
    InvalidParams(String),
    Internal(String),
}

impl From<String> for SearchError {
    fn from(message: String) -> Self {
        Self::Internal(message)
    }
}

impl SearchError {
    pub(crate) fn into_http(self) -> (StatusCode, String) {
        match self {
            Self::InvalidParams(message) => (StatusCode::BAD_REQUEST, message),
            Self::Internal(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
        }
    }
}

/// The stores recall reads; the daemon implements it on `ServiceContext`, tests on a seeded store.
pub(crate) trait RecallBackend {
    fn recall(&self, q: &SearchQuery, cfg: &SearchConfig) -> Result<Vec<SearchHit>, String>;
    fn legacy_rerank(&self, q: &SearchQuery, cfg: &RerankConfig) -> Result<Vec<SearchHit>, String>;
    fn plan_hits(&self, project: &str, q: &PlanIndexQuery) -> Result<Vec<PlanSearchHit>, String>;
}

impl RecallBackend for ServiceContext {
    fn recall(&self, q: &SearchQuery, cfg: &SearchConfig) -> Result<Vec<SearchHit>, String> {
        self.mem.recall_with_config(q, cfg)
    }

    fn legacy_rerank(&self, q: &SearchQuery, cfg: &RerankConfig) -> Result<Vec<SearchHit>, String> {
        self.mem.search_with_rerank(q, cfg)
    }

    fn plan_hits(&self, project: &str, q: &PlanIndexQuery) -> Result<Vec<PlanSearchHit>, String> {
        let project_root = self
            .ensure_plan_project_loaded(std::path::Path::new(project))
            .map_err(|e| e.to_string())?;
        self.plan_store.search_plan_index(&project_root, q)
    }
}

pub async fn mem_search(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    if p.q.trim().is_empty() {
        return Err((
            StatusCode::BAD_REQUEST,
            "search query must not be empty".into(),
        ));
    }
    let svc = state.svc.clone();
    run_blocking_search(move || execute_search(&svc, p))
        .await
        .map(Json)
}

/// Recall and stage 2 block on SQLite and adapter IO, so they never run on the async runtime.
pub(crate) async fn run_blocking_search<F>(
    search: F,
) -> Result<serde_json::Value, (StatusCode, String)>
where
    F: FnOnce() -> Result<serde_json::Value, SearchError> + Send + 'static,
{
    tokio::task::spawn_blocking(search)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("mem search task failed: {error}"),
            )
        })?
        .map_err(SearchError::into_http)
}

pub(crate) fn execute_search(
    svc: &ServiceState,
    p: SearchParams,
) -> Result<serde_json::Value, SearchError> {
    let cfg = crate::config::memory_config().decision_rerank;
    resolve_search(&**svc, &svc.decision_models, &cfg, p)
}

pub(crate) fn resolve_search<B: DecisionBackend + ?Sized>(
    backend: &B,
    models: &DecisionRegistry,
    cfg: &DecisionRerankConfig,
    p: SearchParams,
) -> Result<serde_json::Value, SearchError> {
    let legacy = p.rerank == Some(true);
    if legacy && p.decision_rerank == Some(true) {
        return Err(SearchError::InvalidParams(RERANK_EXCLUSIVE.into()));
    }
    if !p.decision_rerank.unwrap_or(cfg.enabled) {
        return Ok(baseline_search(backend, &p)?);
    }
    if legacy {
        return decision::skipped_search(backend, &p);
    }
    decision::decision_search(backend, models, cfg, &p)
}

/// Today's recall path; feature-off and every fallback return exactly this.
pub(crate) fn baseline_search<B: RecallBackend + ?Sized>(
    backend: &B,
    p: &SearchParams,
) -> Result<serde_json::Value, String> {
    validate_search_query(&p.q)?;
    let q = SearchQuery {
        query: p.q.clone(),
        kind: p.kind.clone(),
        scope: p.scope.clone(),
        area: p.area.clone(),
        limit: p.limit,
    };
    let project = p.project.clone();
    let hits = if p.rerank.unwrap_or(false) {
        let cfg = RerankConfig {
            enabled: true,
            provider: "auto".into(),
            api_key_env: None,
            model: None,
        };
        backend.legacy_rerank(&q, &cfg)?
    } else {
        let mut sc = SearchConfig::default();
        match p.mode.as_deref() {
            Some("vector_first_rerank") | Some("vector_first") => {
                sc.mode = SearchMode::VectorFirstRerank;
            }
            Some("hybrid_rrf") | Some("rrf") | None => {}
            Some(other) => {
                return Err(format!("unknown mode: {}", other));
            }
        }
        if let Some(w) = p.weight {
            if !(0.0..=1.0).contains(&w) {
                return Err(format!("weight must be 0.0..=1.0, got {}", w));
            }
            sc.vector_rerank_weight = w;
        }
        backend.recall(&q, &sc)?
    };
    let mut tagged_hits = hits
        .into_iter()
        .map(|hit| {
            let mut value = serde_json::to_value(hit).unwrap_or_default();
            if let Some(obj) = value.as_object_mut() {
                obj.insert("source".into(), serde_json::json!("mem"));
            }
            value
        })
        .collect::<Vec<_>>();
    if let Some(project) = project {
        append_plan_hits(backend, &mut tagged_hits, &project, &q)?;
    }
    Ok(serde_json::json!({ "hits": tagged_hits }))
}

fn validate_search_query(query: &str) -> Result<(), String> {
    if query.trim().is_empty() {
        Err("search query must not be empty".into())
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod query_tests {
    use super::validate_search_query;

    #[test]
    fn empty_search_query_is_rejected() {
        assert_eq!(
            validate_search_query(" ").unwrap_err(),
            "search query must not be empty"
        );
    }
}

const PLAN_INDEX_LIMIT_CAP: usize = 50;

// Over-fetch plan-index hits (5x) but cap at PLAN_INDEX_LIMIT_CAP; never invert Ord::clamp bounds.
fn effective_plan_index_limit(limit: usize) -> usize {
    limit
        .saturating_mul(5)
        .min(PLAN_INDEX_LIMIT_CAP)
        .max(limit.min(PLAN_INDEX_LIMIT_CAP))
}

fn append_plan_hits<B: RecallBackend + ?Sized>(
    backend: &B,
    tagged_hits: &mut Vec<serde_json::Value>,
    project: &str,
    q: &SearchQuery,
) -> Result<(), String> {
    let mut plan_query = PlanIndexQuery {
        query: q.query.clone(),
        kind: q.kind.clone(),
        scope: q.scope.clone(),
        limit: q.limit,
    };
    if let Some(limit) = q.limit {
        plan_query.limit = Some(effective_plan_index_limit(limit));
    }
    let plan_hits = backend.plan_hits(project, &plan_query)?;
    merge_plan_hits(tagged_hits, plan_hits, q.limit);
    Ok(())
}

fn merge_plan_hits(
    tagged_hits: &mut Vec<serde_json::Value>,
    plan_hits: Vec<PlanSearchHit>,
    limit: Option<usize>,
) {
    if plan_hits.is_empty() {
        if let Some(limit) = limit {
            tagged_hits.truncate(limit);
        }
        return;
    }
    let mut plan_values = plan_hits
        .into_iter()
        .map(|hit| serde_json::to_value(hit).unwrap_or_default())
        .collect::<Vec<_>>();
    sort_hits(&mut plan_values);
    tagged_hits.extend(plan_values.clone());
    sort_hits(tagged_hits);
    let Some(limit) = limit else {
        return;
    };
    tagged_hits.truncate(limit);
    if tagged_hits.iter().any(is_plan_hit) {
        return;
    }
    if limit == 0 {
        return;
    }
    if let Some(best_plan) = plan_values.into_iter().next() {
        if tagged_hits.len() == limit {
            tagged_hits.pop();
        }
        tagged_hits.push(best_plan);
        sort_hits(tagged_hits);
    }
}

fn sort_hits(hits: &mut [serde_json::Value]) {
    hits.sort_by(|a, b| {
        let a_source = a.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let b_source = b.get("source").and_then(|v| v.as_str()).unwrap_or("");
        let a_score = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_score = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        a_score
            .partial_cmp(&b_score)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a_source.cmp(b_source))
    });
}

fn is_plan_hit(hit: &serde_json::Value) -> bool {
    hit.get("source").and_then(|v| v.as_str()) == Some("plan")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(source: &str, score: f64, id: &str) -> serde_json::Value {
        serde_json::json!({ "source": source, "score": score, "id": id })
    }

    #[test]
    fn merge_keeps_project_plan_hit_when_mem_fills_limit() {
        let mut mem_hits = vec![
            hit("mem", 0.01, "m1"),
            hit("mem", 0.02, "m2"),
            hit("mem", 0.03, "m3"),
        ];
        let plan_hit = crate::service::plan::search_index::PlanSearchHit {
            source: "plan",
            score: 0.50,
            id: "p1".into(),
            kind: "task".into(),
            title: "Plan task".into(),
            body: "plan button soft-lock".into(),
            scope: "plan_scope".into(),
            updated_at: 1234,
            age_ms: 5678,
            plan_id: Some("pl_1".into()),
            spec_id: None,
            unit_id: "task_1".into(),
            unit_title: "Task".into(),
            parent_title: "Plan".into(),
            status: Some("pending".into()),
        };

        merge_plan_hits(&mut mem_hits, vec![plan_hit], Some(3));

        assert_eq!(mem_hits.len(), 3);
        assert!(mem_hits.iter().any(is_plan_hit));
    }

    // Pre-fix value for limit<=50: min(limit*5, 50), and always >= limit.
    fn legacy_small_limit(limit: usize) -> usize {
        (limit * 5).min(PLAN_INDEX_LIMIT_CAP)
    }

    #[test]
    fn large_limit_is_bounded_and_never_panics() {
        for limit in [51usize, 60, 100] {
            let eff = effective_plan_index_limit(limit);
            assert!(eff <= PLAN_INDEX_LIMIT_CAP, "limit={limit} eff={eff} > cap");
            assert_eq!(eff, PLAN_INDEX_LIMIT_CAP, "limit>50 must saturate to cap");
        }
    }

    #[test]
    fn small_limit_matches_pre_fix_value() {
        for limit in [1usize, 10, 50] {
            let eff = effective_plan_index_limit(limit);
            assert_eq!(
                eff,
                legacy_small_limit(limit),
                "limit={limit} changed behavior"
            );
            assert!(
                eff >= limit,
                "limit={limit} eff={eff} dropped below user limit"
            );
        }
    }

    #[test]
    fn effective_limit_invariant_over_range() {
        for limit in 1..=1000usize {
            let eff = effective_plan_index_limit(limit);
            let lower = limit.min(PLAN_INDEX_LIMIT_CAP);
            assert!(
                (lower..=PLAN_INDEX_LIMIT_CAP).contains(&eff),
                "limit={limit} eff={eff} out of [{lower}..={PLAN_INDEX_LIMIT_CAP}]"
            );
        }
    }
}
