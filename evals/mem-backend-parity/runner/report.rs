use codeterm_mem::{
    backend::MemStore,
    embedder::{recall_priority::with_recall_status, ActiveEmbedder},
    search::SearchConfig,
    SearchHit, SearchQuery,
};
use serde::Serialize;
use std::{collections::HashSet, sync::Arc};

use super::fixture::Query;

pub const JACCARD_THRESHOLD: f64 = 0.7;

#[derive(Serialize)]
pub struct BackendResult {
    pub top10: Vec<SearchHit>,
    pub exact_title_rank_top10: Option<usize>,
    pub exact_title_rank_full: Option<usize>,
    pub errors: Vec<String>,
}

#[derive(Serialize)]
pub struct QueryResult {
    pub id: String,
    pub query: String,
    pub expected_title: String,
    pub area: Option<String>,
    pub kind: Option<String>,
    pub jaccard_top10: Option<f64>,
    pub missing_from_postgres: Vec<String>,
    pub missing_from_sqlite: Vec<String>,
    pub sqlite: BackendResult,
    pub postgres: BackendResult,
    pub failure_reasons: Vec<String>,
}

fn retrieve(store: &dyn MemStore, query: &Query, embedder: &Arc<ActiveEmbedder>) -> BackendResult {
    let mut result = BackendResult {
        top10: Vec::new(),
        exact_title_rank_top10: None,
        exact_title_rank_full: None,
        errors: Vec::new(),
    };
    for limit in [10, 50] {
        let q = SearchQuery {
            query: query.text.clone(),
            area: query.area.clone(),
            kind: query.kind.clone(),
            scope: Some("global".into()),
            limit: Some(limit),
        };
        let (hits, degraded) =
            with_recall_status(|| store.recall(&q, &SearchConfig::default(), embedder, false));
        if degraded {
            result.errors.push(format!(
                "limit {limit}: query vector unavailable; lexical fallback observed"
            ));
        }
        match hits {
            Ok(hits) => {
                let unique: HashSet<_> = hits.iter().map(|hit| &hit.item.id).collect();
                if unique.len() != hits.len()
                    || hits.len() > limit
                    || hits.iter().any(|hit| !hit.score.is_finite())
                {
                    result
                        .errors
                        .push(format!("limit {limit}: invalid recall result"));
                }
                let rank = hits
                    .iter()
                    .position(|hit| hit.item.title == query.exact_title)
                    .map(|index| index + 1);
                if limit == 10 {
                    result.exact_title_rank_top10 = rank;
                    result.top10 = hits;
                } else {
                    result.exact_title_rank_full = rank;
                }
            }
            Err(error) => result.errors.push(format!("limit {limit}: {error}")),
        }
    }
    result
}

pub fn measure(
    sqlite: &dyn MemStore,
    postgres: &dyn MemStore,
    queries: &[Query],
    embedder: &Arc<ActiveEmbedder>,
) -> Vec<QueryResult> {
    queries
        .iter()
        .map(|query| {
            let sqlite = retrieve(sqlite, query, embedder);
            let postgres = retrieve(postgres, query, embedder);
            let left: HashSet<_> = sqlite.top10.iter().map(|hit| &hit.item.id).collect();
            let right: HashSet<_> = postgres.top10.iter().map(|hit| &hit.item.id).collect();
            let mut missing_from_postgres: Vec<_> =
                left.difference(&right).map(|id| (*id).clone()).collect();
            let mut missing_from_sqlite: Vec<_> =
                right.difference(&left).map(|id| (*id).clone()).collect();
            missing_from_postgres.sort();
            missing_from_sqlite.sort();
            let jaccard = if sqlite.errors.is_empty() && postgres.errors.is_empty() {
                let union = left.union(&right).count();
                Some(if union == 0 {
                    0.0
                } else {
                    left.intersection(&right).count() as f64 / union as f64
                })
            } else {
                None
            };
            let mut reasons = Vec::new();
            if jaccard.is_none() {
                reasons.push("recall error or degraded query vector".into());
            }
            if jaccard.is_some_and(|score| score < JACCARD_THRESHOLD) {
                reasons.push("per-query top-10 Jaccard < 0.7 (diagnostic)".into());
            }
            if sqlite.exact_title_rank_top10 != Some(1) {
                reasons.push("SQLite exact title is not rank 1".into());
            }
            if postgres.exact_title_rank_top10 != Some(1) {
                reasons.push("Postgres exact title is not rank 1".into());
            }
            let row = QueryResult {
                id: query.id.clone(),
                query: query.text.clone(),
                expected_title: query.exact_title.clone(),
                area: query.area.clone(),
                kind: query.kind.clone(),
                jaccard_top10: jaccard,
                missing_from_postgres,
                missing_from_sqlite,
                sqlite,
                postgres,
                failure_reasons: reasons,
            };
            println!(
                "QUERY {}",
                serde_json::to_string(&row).unwrap_or_else(|e| format!("serialization error: {e}"))
            );
            row
        })
        .collect()
}

pub fn summary(rows: &[QueryResult]) -> serde_json::Value {
    let mut jaccards: Option<Vec<f64>> = rows.iter().map(|row| row.jaccard_top10).collect();
    let median = jaccards.as_mut().map(|scores| {
        scores.sort_by(f64::total_cmp);
        let middle = scores.len() / 2;
        if scores.len() % 2 == 0 {
            (scores[middle - 1] + scores[middle]) / 2.0
        } else {
            scores[middle]
        }
    });
    let sqlite = rows
        .iter()
        .filter(|row| row.sqlite.exact_title_rank_top10 == Some(1))
        .count();
    let postgres = rows
        .iter()
        .filter(|row| row.postgres.exact_title_rank_top10 == Some(1))
        .count();
    let both = rows
        .iter()
        .filter(|row| {
            row.sqlite.exact_title_rank_top10 == Some(1)
                && row.postgres.exact_title_rank_top10 == Some(1)
        })
        .count();
    let total = rows.len();
    serde_json::json!({
        "queries": total,
        "provisional_thresholds": {"median_top10_jaccard_min": JACCARD_THRESHOLD, "exact_title_rank": 1},
        "median_top10_jaccard": median,
        "exact_title_rank1_count": {"sqlite": sqlite, "postgres": postgres, "both": both},
        "exact_title_rank1_rate": {
            "sqlite": sqlite as f64 / total as f64,
            "postgres": postgres as f64 / total as f64,
            "both": both as f64 / total as f64,
        },
        "failing_queries": rows.iter().filter(|row| !row.failure_reasons.is_empty()).collect::<Vec<_>>(),
        "passed": median.is_some_and(|value| value >= JACCARD_THRESHOLD) && both == total,
    })
}
