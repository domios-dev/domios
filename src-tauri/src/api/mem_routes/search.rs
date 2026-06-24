use axum::{
    extract::{Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use serde::Deserialize;

use crate::api::routes::{check_auth, AppState};
use crate::mem::{
    rerank::RerankConfig,
    search::{SearchConfig, SearchMode, SearchQuery},
};

#[derive(Deserialize)]
pub struct SearchParams {
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
    pub project: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub weight: Option<f64>,
}

pub async fn mem_search(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Query(p): Query<SearchParams>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    check_auth(&state, &headers).map_err(|(s, e)| (s, e.to_string()))?;
    let q = SearchQuery {
        query: p.q,
        kind: p.kind,
        scope: p.scope,
        area: p.area,
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
        state
            .svc
            .mem
            .search_with_rerank(&q, &cfg)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
    } else {
        let mut sc = SearchConfig::default();
        match p.mode.as_deref() {
            Some("vector_first_rerank") | Some("vector_first") => {
                sc.mode = SearchMode::VectorFirstRerank;
            }
            Some("hybrid_rrf") | Some("rrf") | None => {}
            Some(other) => {
                return Err((StatusCode::BAD_REQUEST, format!("unknown mode: {}", other)));
            }
        }
        if let Some(w) = p.weight {
            if !(0.0..=1.0).contains(&w) {
                return Err((
                    StatusCode::BAD_REQUEST,
                    format!("weight must be 0.0..=1.0, got {}", w),
                ));
            }
            sc.vector_rerank_weight = w;
        }
        state
            .svc
            .mem
            .recall_with_config(&q, &sc)
            .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?
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
        append_plan_hits(&state, &mut tagged_hits, &project, &q)?;
    }
    Ok(Json(serde_json::json!({ "hits": tagged_hits })))
}

fn append_plan_hits(
    state: &AppState,
    tagged_hits: &mut Vec<serde_json::Value>,
    project: &str,
    q: &SearchQuery,
) -> Result<(), (StatusCode, String)> {
    let project_root = state
        .svc
        .ensure_plan_project_loaded(std::path::Path::new(project))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let plan_hits = state
        .svc
        .plan_store
        .search_plan_index(&project_root, q)
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e))?;
    tagged_hits.extend(
        plan_hits
            .into_iter()
            .map(|hit| serde_json::to_value(hit).unwrap_or_default()),
    );
    tagged_hits.sort_by(|a, b| {
        let a_score = a.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let b_score = b.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0);
        a_score
            .partial_cmp(&b_score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if let Some(limit) = q.limit {
        tagged_hits.truncate(limit);
    }
    Ok(())
}
