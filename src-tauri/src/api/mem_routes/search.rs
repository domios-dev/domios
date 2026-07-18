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
use crate::service::ServiceState;

#[derive(Deserialize)]
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
    execute_search(&state.svc, p)
        .map(Json)
        .map_err(|message| (StatusCode::INTERNAL_SERVER_ERROR, message))
}

pub(crate) fn execute_search(
    svc: &ServiceState,
    p: SearchParams,
) -> Result<serde_json::Value, String> {
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
        svc.mem.search_with_rerank(&q, &cfg)?
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
        svc.mem.recall_with_config(&q, &sc)?
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
        append_plan_hits(svc, &mut tagged_hits, &project, &q)?;
    }
    Ok(serde_json::json!({ "hits": tagged_hits }))
}

fn append_plan_hits(
    svc: &ServiceState,
    tagged_hits: &mut Vec<serde_json::Value>,
    project: &str,
    q: &SearchQuery,
) -> Result<(), String> {
    use crate::service::plan::ports::PlanIndexQuery;
    let project_root = svc
        .ensure_plan_project_loaded(std::path::Path::new(project))
        .map_err(|e| e.to_string())?;
    let mut plan_query = PlanIndexQuery {
        query: q.query.clone(),
        scope: q.scope.clone(),
        limit: q.limit,
    };
    if let Some(limit) = q.limit {
        plan_query.limit = Some((limit * 5).clamp(limit, 50));
    }
    let plan_hits = svc
        .plan_store
        .search_plan_index(&project_root, &plan_query)?;
    merge_plan_hits(tagged_hits, plan_hits, q.limit);
    Ok(())
}

fn merge_plan_hits(
    tagged_hits: &mut Vec<serde_json::Value>,
    plan_hits: Vec<crate::service::plan::search_index::PlanSearchHit>,
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
}
