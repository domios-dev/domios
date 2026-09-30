//! WHAT: Compare stage-2 decision ordering with the ordinary recall order.
//! WHY: The optional quality probe must leave the established bench oracle intact.
//! ORACLE: Off reports have no decision key; enabled reports score both orders and absent answers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_lib::mem::decision::{
    rerank::{run_stage, RerankCandidate, StageConfig, StageOutcome},
    token_store::TokenStore,
    typesafe::TypeSafeAdapter,
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionQuestion,
    DecisionRequest,
};
use codeterm_lib::mem::{MemManager, SearchQuery};
use serde::{Deserialize, Serialize};

use super::{Fixture, TestCase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode { Off, Stub, Live }

impl Mode {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value { "off" => Ok(Self::Off), "stub" => Ok(Self::Stub), "live" => Ok(Self::Live), _ => Err(format!("invalid --decision-rerank mode: {value} (expected off|stub|live)")) }
    }
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub(super) struct RankingMetrics {
    pub recall_at_1: f64,
    pub recall_at_k: f64,
    pub mrr: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct NegativeMetrics {
    pub count: usize,
    pub low_confidence_count: usize,
    pub low_confidence_precision: f64,
    pub positive_low_confidence_count: usize,
    pub positive_low_confidence_false_positive_rate: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct DecisionQuery {
    pub test_id: String,
    pub stage_latency_ms: f64,
    pub fallback: Option<String>,
    pub low_confidence: Option<bool>,
    pub baseline_rank: Option<usize>,
    pub reranked_rank: Option<usize>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct DecisionReport {
    pub mode: String,
    pub baseline: RankingMetrics,
    pub reranked: RankingMetrics,
    pub fallbacks: BTreeMap<String, usize>,
    pub negatives: NegativeMetrics,
    pub per_query: Vec<DecisionQuery>,
}

#[derive(Deserialize)]
struct NegativeFixture { test_cases: Vec<NegativeCase> }

#[derive(Deserialize)]
struct NegativeCase { id: String, query: String, expect_none: bool }

struct StubModel;

impl DecisionModel for StubModel {
    fn id(&self) -> &str { "bench-keyword-stub" }
    fn primitives(&self) -> DecisionPrimitives { DecisionPrimitives { noul: true, ..Default::default() } }
    fn decide(&self, req: &DecisionRequest, _deadline: Instant) -> Result<DecisionAnswer, DecisionError> {
        if req.question != DecisionQuestion::Noul { return Err(DecisionError::Unsupported); }
        let (query, candidate) = req.state.strip_prefix("Query:\n")
            .and_then(|s| s.split_once("\n\nCandidate:\n"))
            .ok_or_else(|| DecisionError::Parse("invalid_bench_request".into()))?;
        let query_words = words(query);
        let candidate_words = words(candidate);
        let overlap = query_words.intersection(&candidate_words).count();
        let p = if query_words.is_empty() { 0.0 } else { overlap as f64 / query_words.len() as f64 };
        Ok(DecisionAnswer::Noul { p })
    }
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.chars().count() > 2)
        .map(str::to_lowercase)
        .collect()
}

pub(super) fn prepare_model(mode: Mode, data_dir: Option<&Path>) -> Result<Option<Arc<dyn DecisionModel>>, String> {
    match mode {
        Mode::Off => Ok(None),
        Mode::Stub => Ok(Some(Arc::new(StubModel))),
        Mode::Live => {
            let root = data_dir.map(Path::to_path_buf).unwrap_or_else(codeterm_lib::paths::data_dir);
            let store = TokenStore::new(root.join("decision-models"));
            let present = store.load(TypeSafeAdapter::ID)
                .map_err(|_| "decision live token unreadable or invalid".to_string())?
                .is_some();
            if !present { return Err("decision live token missing; store it before running --decision-rerank live".into()); }
            Ok(Some(Arc::new(TypeSafeAdapter::new(store, None, None))))
        }
    }
}

fn rank(ids: &[String], expected: &[String]) -> Option<usize> {
    ids.iter().position(|id| expected.contains(id)).map(|i| i + 1)
}

fn ranking_metrics(ranks: &[Option<usize>], k: usize) -> RankingMetrics {
    let n = ranks.len().max(1) as f64;
    RankingMetrics {
        recall_at_1: ranks.iter().filter(|r| **r == Some(1)).count() as f64 / n,
        recall_at_k: ranks.iter().filter(|r| r.is_some_and(|v| v <= k)).count() as f64 / n,
        mrr: ranks.iter().filter_map(|r| r.map(|v| 1.0 / v as f64)).sum::<f64>() / n,
    }
}

fn negative_metrics(negative_flags: &[bool], positive_flags: &[bool]) -> NegativeMetrics {
    let low = negative_flags.iter().filter(|v| **v).count();
    let positive_low = positive_flags.iter().filter(|v| **v).count();
    let all_low = low + positive_low;
    NegativeMetrics {
        count: negative_flags.len(), low_confidence_count: low,
        low_confidence_precision: if all_low == 0 { 0.0 } else { low as f64 / all_low as f64 },
        positive_low_confidence_count: positive_low,
        positive_low_confidence_false_positive_rate: positive_low as f64 / positive_flags.len().max(1) as f64,
    }
}

pub(super) fn evaluate(
    mode: Mode, model: Arc<dyn DecisionModel>, fixture: &Fixture, fixture_path: &Path,
    mem: &MemManager, id_map: &HashMap<String, String>, top_k: usize,
) -> Result<DecisionReport, String> {
    let negatives_path = fixture_path.parent().unwrap_or_else(|| Path::new(".")) .join("decision_negatives.json");
    let raw = fs::read_to_string(&negatives_path).map_err(|e| format!("read {}: {e}", negatives_path.display()))?;
    let negatives: NegativeFixture = serde_json::from_str(&raw).map_err(|e| format!("parse decision negatives: {e}"))?;
    if negatives.test_cases.len() < 8 || negatives.test_cases.iter().any(|c| !c.expect_none) {
        return Err("decision negatives must contain at least 8 expect_none cases".into());
    }
    let cfg = StageConfig { max_concurrency: 4, deadline: Duration::from_secs(10), low_confidence_threshold: 0.25, max_body_chars: 2000 };
    let mut baseline_ranks = Vec::new();
    let mut reranked_ranks = Vec::new();
    let mut negative_flags = Vec::new();
    let mut positive_flags = Vec::new();
    let mut fallbacks = BTreeMap::new();
    let mut per_query = Vec::new();
    let cases = fixture.test_cases.iter().map(|tc| (tc.id.as_str(), tc.query.as_str(), Some(tc)))
        .chain(negatives.test_cases.iter().map(|tc| (tc.id.as_str(), tc.query.as_str(), None)));
    for (id, query, positive) in cases {
        let hits = mem.recall(&SearchQuery { query: query.into(), kind: None, scope: None, area: None, limit: Some(top_k.max(10)) })
            .map_err(|e| format!("decision recall {id}: {e}"))?;
        let candidates: Vec<_> = hits.iter().map(|h| RerankCandidate {
            key: h.item.id.clone(),
            text: format!("{}\n{}\n{}", h.item.title, h.item.summary.as_deref().unwrap_or(""), h.item.body),
        }).collect();
        let started = Instant::now();
        let outcome = run_stage(model.clone(), query, &candidates, &cfg);
        let stage_latency_ms = started.elapsed().as_secs_f64() * 1000.0;
        let baseline_ids: Vec<String> = hits.iter().filter_map(|h| id_map.get(&h.item.id).cloned()).take(top_k).collect();
        let (order, fallback, low_confidence) = match outcome {
            StageOutcome::Applied { order, low_confidence, .. } => (order, None, Some(low_confidence)),
            StageOutcome::Fallback { reason } => {
                *fallbacks.entry(reason.as_str().to_owned()).or_insert(0) += 1;
                ((0..hits.len()).collect(), Some(reason.as_str().to_owned()), None)
            }
        };
        let reranked_ids: Vec<String> = order.iter().filter_map(|&i| id_map.get(&hits[i].item.id).cloned()).take(top_k).collect();
        let expected = positive.map(TestCase::expected_set).unwrap_or_default();
        let baseline_rank = rank(&baseline_ids, &expected);
        let reranked_rank = rank(&reranked_ids, &expected);
        if positive.is_some() {
            baseline_ranks.push(baseline_rank);
            reranked_ranks.push(reranked_rank);
            positive_flags.push(low_confidence.unwrap_or(false));
        } else { negative_flags.push(low_confidence.unwrap_or(false)); }
        per_query.push(DecisionQuery { test_id: id.into(), stage_latency_ms, fallback, low_confidence, baseline_rank, reranked_rank });
    }
    Ok(DecisionReport {
        mode: match mode { Mode::Stub => "stub", Mode::Live => "live", Mode::Off => unreachable!() }.into(),
        baseline: ranking_metrics(&baseline_ranks, top_k), reranked: ranking_metrics(&reranked_ranks, top_k),
        fallbacks, negatives: negative_metrics(&negative_flags, &positive_flags), per_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decision_metrics_mrr_and_recall_math() {
        let metrics = ranking_metrics(&[Some(1), Some(3), None, Some(2)], 2);
        assert_eq!(metrics.recall_at_1, 0.25);
        assert_eq!(metrics.recall_at_k, 0.5);
        assert!((metrics.mrr - (1.0 + 1.0/3.0 + 0.5)/4.0).abs() < 1e-12);
    }
    #[test]
    fn decision_negative_low_confidence_precision() {
        let metrics = negative_metrics(&[true, true, false], &[true, false]);
        assert_eq!(metrics.low_confidence_precision, 2.0/3.0);
        assert_eq!(metrics.positive_low_confidence_false_positive_rate, 0.5);
    }
}
