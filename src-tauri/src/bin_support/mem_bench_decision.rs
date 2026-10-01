//! WHAT: Compare stage-2 decision ordering with the ordinary recall order.
//! WHY: The optional quality probe must leave the established bench oracle intact.
//! ORACLE: Off reports have no decision key; enabled reports score both orders and absent answers.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fs;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_lib::mem::decision::{
    rerank::{run_stage, RerankCandidate, StageConfig, StageOutcome, NOUL_INSTRUCTIONS_TEMPLATE},
    token_store::TokenStore,
    typesafe::TypeSafeAdapter,
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionQuestion,
    DecisionRequest,
};
use codeterm_lib::mem::{MemManager, SearchHit, SearchQuery};
use serde::{Deserialize, Serialize};

use super::{Fixture, SessionCandidateFixture, TestCase};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Off,
    Stub,
    Live,
}

impl Mode {
    pub(super) fn parse(value: &str) -> Result<Self, String> {
        match value {
            "off" => Ok(Self::Off),
            "stub" => Ok(Self::Stub),
            "live" => Ok(Self::Live),
            _ => Err(format!(
                "invalid --supporter mode: {value} (expected off|stub|live)"
            )),
        }
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
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub graph: Option<GraphQuery>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub docs: Option<DocsQuery>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sessions: Option<SessionsQuery>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct GraphQuery {
    pub rank: Option<usize>,
    pub added: usize,
    pub top1_from_graph: bool,
}

/// The re-rank+graph arm; a graph-only winner is a positive query whose expected
/// answer ranks first and reached the pool only through the graph source.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct GraphArm {
    pub sources_graph: usize,
    pub metrics: RankingMetrics,
    pub added: usize,
    pub graph_only_top1: usize,
    pub graph_only_winners: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct DocsQuery {
    pub rank: Option<usize>,
    pub added: usize,
    pub top1_from_docs: bool,
    pub stage_latency_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct DocsArm {
    pub sources_docs: usize,
    pub hybrid: bool,
    pub metrics: RankingMetrics,
    pub added: usize,
    pub docs_top1: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct SessionsQuery {
    pub rank: Option<usize>,
    pub added: usize,
    pub top1_from_sessions: bool,
    pub stage_latency_ms: f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct SessionsArm {
    pub sources_sessions: usize,
    pub metrics: RankingMetrics,
    pub added: usize,
    pub sessions_top1: usize,
    pub sessions_only_winners: usize,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub(super) struct DecisionReport {
    pub mode: String,
    pub baseline: RankingMetrics,
    pub reranked: RankingMetrics,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub graph: Option<GraphArm>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub docs: Option<DocsArm>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sessions: Option<SessionsArm>,
    pub fallbacks: BTreeMap<String, usize>,
    pub negatives: NegativeMetrics,
    pub per_query: Vec<DecisionQuery>,
}

/// Optional fixture graph: entity ids plus links whose ends are seed ids or entity ids.
#[derive(Debug, Deserialize, Default)]
pub(super) struct FixtureGraph {
    #[serde(default)]
    entities: Vec<String>,
    #[serde(default)]
    links: Vec<FixtureLink>,
}

#[derive(Debug, Deserialize)]
struct FixtureLink {
    from: String,
    to: String,
}

/// Raw SQL entities: `graph_create_entity` would embed, and default recall ignores the graph.
pub(super) fn seed_graph(
    mem: &MemManager,
    graph: &FixtureGraph,
    engine_ids: &HashMap<String, String>,
) -> Result<(), String> {
    let resolve = |id: &String| engine_ids.get(id).unwrap_or(id).clone();
    mem.with_conn(|conn| {
        for id in &graph.entities {
            conn.execute(
                "INSERT OR IGNORE INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
                 VALUES (?1, 'Concept', 'global', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [id],
            )
            .map_err(|e| format!("seed entity {id}: {e}"))?;
        }
        for link in &graph.links {
            codeterm_lib::mem::graph::insert_relation(
                conn,
                &codeterm_lib::mem::Relation {
                    from_id: resolve(&link.from),
                    relation_type: "mentions".into(),
                    to_id: resolve(&link.to),
                    scope: "global".into(),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: None,
                    confidence: 1.0,
                    properties: serde_json::json!({}),
                    reason: None,
                },
            )?;
        }
        Ok(())
    })
}

#[derive(Deserialize)]
struct NegativeFixture {
    test_cases: Vec<NegativeCase>,
}

#[derive(Deserialize)]
struct NegativeCase {
    id: String,
    query: String,
    expect_none: bool,
    #[serde(default)]
    session_candidates: Vec<SessionCandidateFixture>,
}

struct StubModel;

impl DecisionModel for StubModel {
    fn id(&self) -> &str {
        "bench-keyword-stub"
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..Default::default()
        }
    }
    fn decide(
        &self,
        req: &DecisionRequest,
        _deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        if !matches!(&req.question, DecisionQuestion::Noul { .. }) {
            return Err(DecisionError::Unsupported);
        }
        let Some((prefix, suffix)) = NOUL_INSTRUCTIONS_TEMPLATE.split_once("{query}") else {
            return Err(DecisionError::Parse("invalid_bench_template".into()));
        };
        let query = req
            .instructions
            .strip_prefix(prefix)
            .and_then(|s| s.strip_suffix(suffix))
            .ok_or_else(|| DecisionError::Parse("invalid_bench_instructions".into()))?;
        let memory = req
            .state
            .as_str()
            .and_then(|state| state.strip_prefix("MEMORY:\n"))
            .and_then(|state| state.split_once('\n'))
            .ok_or_else(|| DecisionError::Parse("invalid_bench_request".into()))?;
        let query_words = words(query);
        let candidate_words = words(&format!("{}\n{}", memory.0, memory.1));
        let overlap = query_words.intersection(&candidate_words).count();
        let p = if query_words.is_empty() {
            0.0
        } else {
            overlap as f64 / query_words.len() as f64
        };
        Ok(DecisionAnswer::Noul { p })
    }
}

fn words(text: &str) -> BTreeSet<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|s| s.chars().count() > 2)
        .map(str::to_lowercase)
        .collect()
}

pub(super) fn prepare_model(
    mode: Mode,
    data_dir: Option<&Path>,
) -> Result<Option<Arc<dyn DecisionModel>>, String> {
    match mode {
        Mode::Off => Ok(None),
        Mode::Stub => Ok(Some(Arc::new(StubModel))),
        Mode::Live => {
            let root = data_dir
                .map(Path::to_path_buf)
                .unwrap_or_else(codeterm_lib::paths::data_dir);
            let store = TokenStore::new(root.join("supporter"));
            let present = store
                .load(TypeSafeAdapter::ID)
                .map_err(|_| "decision live token unreadable or invalid".to_string())?
                .is_some();
            if !present {
                return Err(
                    "decision live token missing; store it before running --supporter live".into(),
                );
            }
            Ok(Some(Arc::new(TypeSafeAdapter::new(store, None, None))))
        }
    }
}

fn candidate(hit: &SearchHit) -> RerankCandidate {
    RerankCandidate {
        key: hit.item.id.clone(),
        title: hit.item.title.clone(),
        kind: hit.item.kind.clone(),
        body: hit.item.body.clone(),
    }
}

fn doc_candidate(doc: &codeterm_lib::mem::docs_source::DocCandidate) -> RerankCandidate {
    RerankCandidate {
        key: format!("doc:{}#{}", doc.doc_name, doc.anchor),
        title: format!("{} § {}", doc.doc_name, doc.section_title),
        kind: "doc".into(),
        body: doc.text.clone(),
    }
}

fn session_additions(
    query: &str,
    sessions: &[SessionCandidateFixture],
    limit: usize,
) -> Vec<SessionCandidateFixture> {
    if limit == 0 {
        return Vec::new();
    }
    let terms: Vec<String> = query
        .split_whitespace()
        .map(str::to_ascii_lowercase)
        .collect();
    let mut matches: Vec<_> = sessions
        .iter()
        .filter(|session| {
            let excerpt = session.excerpt.to_ascii_lowercase();
            terms.iter().any(|term| excerpt.contains(term))
        })
        .cloned()
        .collect();
    matches.sort_by(|a, b| {
        b.modified_ts
            .unwrap_or(0)
            .cmp(&a.modified_ts.unwrap_or(0))
            .then_with(|| a.provider.cmp(&b.provider))
            .then_with(|| a.session_id.cmp(&b.session_id))
    });
    matches.truncate(limit);
    matches
}

fn session_candidate(session: &SessionCandidateFixture, max_body_chars: usize) -> RerankCandidate {
    let date = session
        .modified_ts
        .map(|timestamp| format!("Unix timestamp {timestamp}"))
        .unwrap_or_else(|| "unknown".into());
    let body = format!(
        "title: {}\nprovider: {}\ndate: {date}\nmatched excerpt: {}",
        session.title, session.provider, session.excerpt
    );
    let body = match body.char_indices().nth(max_body_chars) {
        Some((byte, _)) => body[..byte].to_string(),
        None => body,
    };
    RerankCandidate {
        key: format!("session:{}:{}", session.provider, session.session_id),
        title: session.title.clone(),
        kind: "session".into(),
        body,
    }
}

fn stage_order(outcome: StageOutcome, n: usize) -> Vec<usize> {
    match outcome {
        StageOutcome::Applied { order, .. } => order,
        StageOutcome::Fallback { .. } => (0..n).collect(),
    }
}

/// Up to `m` graph-linked items absent from `pool`, fetched without recording access.
fn graph_additions(
    mem: &MemManager,
    query: &str,
    pool: &[SearchHit],
    m: usize,
) -> Result<Vec<SearchHit>, String> {
    let q = SearchQuery {
        query: query.into(),
        kind: None,
        scope: None,
        area: None,
        limit: None,
    };
    let pooled: BTreeSet<&str> = pool.iter().map(|h| h.item.id.as_str()).collect();
    Ok(mem
        .graph_candidates_untouched(&q, m + pooled.len())?
        .into_iter()
        .filter(|h| !pooled.contains(h.item.id.as_str()))
        .take(m)
        .collect())
}

fn rank(ids: &[String], expected: &[String]) -> Option<usize> {
    ids.iter()
        .position(|id| expected.contains(id))
        .map(|i| i + 1)
}

fn ranking_metrics(ranks: &[Option<usize>], k: usize) -> RankingMetrics {
    let n = ranks.len().max(1) as f64;
    RankingMetrics {
        recall_at_1: ranks.iter().filter(|r| **r == Some(1)).count() as f64 / n,
        recall_at_k: ranks.iter().filter(|r| r.is_some_and(|v| v <= k)).count() as f64 / n,
        mrr: ranks
            .iter()
            .filter_map(|r| r.map(|v| 1.0 / v as f64))
            .sum::<f64>()
            / n,
    }
}

fn negative_metrics(negative_flags: &[bool], positive_flags: &[bool]) -> NegativeMetrics {
    let low = negative_flags.iter().filter(|v| **v).count();
    let positive_low = positive_flags.iter().filter(|v| **v).count();
    let all_low = low + positive_low;
    NegativeMetrics {
        count: negative_flags.len(),
        low_confidence_count: low,
        low_confidence_precision: if all_low == 0 {
            0.0
        } else {
            low as f64 / all_low as f64
        },
        positive_low_confidence_count: positive_low,
        positive_low_confidence_false_positive_rate: positive_low as f64
            / positive_flags.len().max(1) as f64,
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn evaluate(
    mode: Mode,
    model: Arc<dyn DecisionModel>,
    fixture: &Fixture,
    fixture_path: &Path,
    mem: &MemManager,
    id_map: &HashMap<String, String>,
    top_k: usize,
    graph: usize,
    docs: usize,
    docs_hybrid: bool,
    sessions: usize,
) -> Result<DecisionReport, String> {
    let negatives_path = fixture_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("decision_negatives.json");
    let raw = fs::read_to_string(&negatives_path)
        .map_err(|e| format!("read {}: {e}", negatives_path.display()))?;
    let negatives: NegativeFixture =
        serde_json::from_str(&raw).map_err(|e| format!("parse decision negatives: {e}"))?;
    if negatives.test_cases.len() < 8 || negatives.test_cases.iter().any(|c| !c.expect_none) {
        return Err("decision negatives must contain at least 8 expect_none cases".into());
    }
    let cfg = StageConfig {
        max_concurrency: 4,
        deadline: Duration::from_secs(10),
        low_confidence_threshold: 0.25,
        max_body_chars: 2000,
    };
    let mut baseline_ranks = Vec::new();
    let mut reranked_ranks = Vec::new();
    let mut negative_flags = Vec::new();
    let mut positive_flags = Vec::new();
    let mut fallbacks = BTreeMap::new();
    let mut per_query = Vec::new();
    let mut graph_ranks = Vec::new();
    let mut graph_added = 0;
    let mut graph_only_top1 = 0;
    let mut graph_only_winners = 0;
    let mut docs_ranks = Vec::new();
    let mut docs_added = 0;
    let mut docs_top1 = 0;
    let mut sessions_ranks = Vec::new();
    let mut sessions_added = 0;
    let mut sessions_top1 = 0;
    let mut sessions_only_winners = 0;
    let cases = fixture
        .test_cases
        .iter()
        .map(|tc| {
            (
                tc.id.as_str(),
                tc.query.as_str(),
                Some(tc),
                tc.session_candidates.as_slice(),
            )
        })
        .chain(negatives.test_cases.iter().map(|tc| {
            (
                tc.id.as_str(),
                tc.query.as_str(),
                None,
                tc.session_candidates.as_slice(),
            )
        }));
    for (id, query, positive, session_fixture) in cases {
        let hits = mem
            .recall(&SearchQuery {
                query: query.into(),
                kind: None,
                scope: None,
                area: None,
                limit: Some(top_k.max(10)),
            })
            .map_err(|e| format!("decision recall {id}: {e}"))?;
        let candidates: Vec<_> = hits.iter().map(candidate).collect();
        let started = Instant::now();
        let outcome = run_stage(model.clone(), query, &candidates, &cfg);
        let stage_latency_ms = started.elapsed().as_secs_f64() * 1000.0;
        let baseline_ids: Vec<String> = hits
            .iter()
            .filter_map(|h| id_map.get(&h.item.id).cloned())
            .take(top_k)
            .collect();
        let (order, fallback, low_confidence) = match outcome {
            StageOutcome::Applied {
                order,
                low_confidence,
                ..
            } => (order, None, Some(low_confidence)),
            StageOutcome::Fallback { reason } => {
                *fallbacks.entry(reason.as_str().to_owned()).or_insert(0) += 1;
                (
                    (0..hits.len()).collect(),
                    Some(reason.as_str().to_owned()),
                    None,
                )
            }
        };
        let reranked_ids: Vec<String> = order
            .iter()
            .filter_map(|&i| id_map.get(&hits[i].item.id).cloned())
            .take(top_k)
            .collect();
        let expected = positive.map(TestCase::expected_set).unwrap_or_default();
        let baseline_rank = rank(&baseline_ids, &expected);
        let reranked_rank = rank(&reranked_ids, &expected);
        let graph_query = if graph > 0 {
            let additions = graph_additions(mem, query, &hits, graph)
                .map_err(|e| format!("decision graph {id}: {e}"))?;
            let (graph_rank, top1_from_graph) = if additions.is_empty() {
                (reranked_rank, false)
            } else {
                let pool: Vec<&SearchHit> = hits.iter().chain(&additions).collect();
                let pooled: Vec<_> = pool.iter().map(|h| candidate(h)).collect();
                let order = stage_order(run_stage(model.clone(), query, &pooled, &cfg), pool.len());
                let ids: Vec<String> = order
                    .iter()
                    .filter_map(|&i| id_map.get(&pool[i].item.id).cloned())
                    .take(top_k)
                    .collect();
                let top1_from_graph = order.first().is_some_and(|&i| i >= hits.len());
                (rank(&ids, &expected), top1_from_graph)
            };
            graph_added += additions.len();
            if top1_from_graph {
                graph_only_top1 += 1;
                if positive.is_some() && graph_rank == Some(1) {
                    graph_only_winners += 1;
                }
            }
            if positive.is_some() {
                graph_ranks.push(graph_rank);
            }
            Some(GraphQuery {
                rank: graph_rank,
                added: additions.len(),
                top1_from_graph,
            })
        } else {
            None
        };
        let docs_query = if docs > 0 {
            let additions =
                codeterm_lib::mem::docs_source::search_with_hybrid(query, docs, docs_hybrid);
            let pool_len = hits.len() + additions.len();
            let candidates = hits
                .iter()
                .map(candidate)
                .chain(additions.iter().map(doc_candidate))
                .collect::<Vec<_>>();
            let started = Instant::now();
            let order = stage_order(run_stage(model.clone(), query, &candidates, &cfg), pool_len);
            let docs_stage_latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let ids = order
                .iter()
                .filter_map(|&index| {
                    hits.get(index)
                        .and_then(|hit| id_map.get(&hit.item.id).cloned())
                })
                .take(top_k)
                .collect::<Vec<_>>();
            let docs_rank = rank(&ids, &expected);
            let top1_from_docs = order.first().is_some_and(|&index| index >= hits.len());
            docs_added += additions.len();
            if top1_from_docs {
                docs_top1 += 1;
            }
            if positive.is_some() {
                docs_ranks.push(docs_rank);
            }
            Some(DocsQuery {
                rank: docs_rank,
                added: additions.len(),
                top1_from_docs,
                stage_latency_ms: docs_stage_latency_ms,
            })
        } else {
            None
        };
        let sessions_query = if sessions > 0 {
            let additions = session_additions(query, session_fixture, sessions);
            let candidates = hits
                .iter()
                .map(candidate)
                .chain(
                    additions
                        .iter()
                        .map(|session| session_candidate(session, cfg.max_body_chars)),
                )
                .collect::<Vec<_>>();
            let started = Instant::now();
            let outcome = run_stage(model.clone(), query, &candidates, &cfg);
            let sessions_stage_latency_ms = started.elapsed().as_secs_f64() * 1000.0;
            let (order, applied) = match outcome {
                StageOutcome::Applied { order, .. } => (order, true),
                StageOutcome::Fallback { .. } => ((0..hits.len()).collect(), false),
            };
            let ids = order
                .iter()
                .filter_map(|&index| {
                    if let Some(hit) = hits.get(index) {
                        id_map.get(&hit.item.id).cloned()
                    } else {
                        additions
                            .get(index.saturating_sub(hits.len()))
                            .map(|session| session.session_id.clone())
                    }
                })
                .take(top_k)
                .collect::<Vec<_>>();
            let sessions_rank = rank(&ids, &expected);
            let top1_from_sessions =
                applied && order.first().is_some_and(|&index| index >= hits.len());
            sessions_added += additions.len();
            if top1_from_sessions {
                sessions_top1 += 1;
                if positive.is_some() && sessions_rank == Some(1) {
                    sessions_only_winners += 1;
                }
            }
            if positive.is_some() {
                sessions_ranks.push(sessions_rank);
            }
            Some(SessionsQuery {
                rank: sessions_rank,
                added: additions.len(),
                top1_from_sessions,
                stage_latency_ms: sessions_stage_latency_ms,
            })
        } else {
            None
        };
        if positive.is_some() {
            baseline_ranks.push(baseline_rank);
            reranked_ranks.push(reranked_rank);
            positive_flags.push(low_confidence.unwrap_or(false));
        } else {
            negative_flags.push(low_confidence.unwrap_or(false));
        }
        per_query.push(DecisionQuery {
            test_id: id.into(),
            stage_latency_ms,
            fallback,
            low_confidence,
            baseline_rank,
            reranked_rank,
            graph: graph_query,
            docs: docs_query,
            sessions: sessions_query,
        });
    }
    Ok(DecisionReport {
        mode: match mode {
            Mode::Stub => "stub",
            Mode::Live => "live",
            Mode::Off => unreachable!(),
        }
        .into(),
        baseline: ranking_metrics(&baseline_ranks, top_k),
        reranked: ranking_metrics(&reranked_ranks, top_k),
        graph: (graph > 0).then(|| GraphArm {
            sources_graph: graph,
            metrics: ranking_metrics(&graph_ranks, top_k),
            added: graph_added,
            graph_only_top1,
            graph_only_winners,
        }),
        docs: (docs > 0).then(|| DocsArm {
            sources_docs: docs,
            hybrid: docs_hybrid,
            metrics: ranking_metrics(&docs_ranks, top_k),
            added: docs_added,
            docs_top1,
        }),
        sessions: (sessions > 0).then(|| SessionsArm {
            sources_sessions: sessions,
            metrics: ranking_metrics(&sessions_ranks, top_k),
            added: sessions_added,
            sessions_top1,
            sessions_only_winners,
        }),
        fallbacks,
        negatives: negative_metrics(&negative_flags, &positive_flags),
        per_query,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decision_stub_reads_memory_string_and_rejects_object_state() {
        let query = "orchard valve";
        let request = DecisionRequest {
            state: serde_json::Value::String("MEMORY:\nOrchard\nValve handbook".into()),
            instructions: NOUL_INSTRUCTIONS_TEMPLATE.replace("{query}", query),
            question: DecisionQuestion::Noul { criteria: None },
        };
        let answer = StubModel.decide(&request, Instant::now()).unwrap();
        assert_eq!(answer, DecisionAnswer::Noul { p: 1.0 });

        let mut object_state = request;
        object_state.state = serde_json::json!({"query": query});
        assert!(matches!(
            StubModel.decide(&object_state, Instant::now()),
            Err(DecisionError::Parse(_))
        ));
    }

    #[test]
    fn decision_metrics_mrr_and_recall_math() {
        let metrics = ranking_metrics(&[Some(1), Some(3), None, Some(2)], 2);
        assert_eq!(metrics.recall_at_1, 0.25);
        assert_eq!(metrics.recall_at_k, 0.5);
        assert!((metrics.mrr - (1.0 + 1.0 / 3.0 + 0.5) / 4.0).abs() < 1e-12);
    }
    #[test]
    fn decision_graph_off_adds_no_graph_keys() {
        let query = DecisionQuery {
            test_id: "T01".into(),
            stage_latency_ms: 0.0,
            fallback: None,
            low_confidence: None,
            baseline_rank: None,
            reranked_rank: None,
            graph: None,
            docs: None,
            sessions: None,
        };
        assert!(serde_json::to_value(&query).unwrap().get("graph").is_none());
        assert!(serde_json::to_value(&query).unwrap().get("docs").is_none());
        assert!(serde_json::to_value(&query)
            .unwrap()
            .get("sessions")
            .is_none());
    }

    #[test]
    fn decision_doc_candidate_can_win_under_the_stub() {
        let section = codeterm_lib::mem::docs_source::DocCandidate {
            doc_name: "SECRETS",
            section_title: "Vault unlock".into(),
            anchor: "vault-unlock".into(),
            text: "Unlock the vault with the stored secret.".into(),
            relevance: 1.0,
        };
        let candidate = doc_candidate(&section);
        let unrelated = RerankCandidate {
            key: "memory-1".into(),
            title: "Terminal colors".into(),
            kind: "fact".into(),
            body: "The terminal background is blue.".into(),
        };

        let outcome = run_stage(
            Arc::new(StubModel),
            "unlock vault secret",
            &[unrelated, candidate.clone()],
            &StageConfig {
                max_concurrency: 1,
                deadline: Duration::from_secs(1),
                low_confidence_threshold: 0.15,
                max_body_chars: 2000,
            },
        );

        assert_eq!(candidate.kind, "doc");
        assert!(matches!(outcome, StageOutcome::Applied { order, .. } if order == vec![1, 0]));
    }

    #[test]
    fn decision_session_candidates_match_any_term_and_use_recent_order() {
        let fixtures = vec![
            SessionCandidateFixture {
                session_id: "older".into(),
                provider: "claude".into(),
                title: "Deploy notes".into(),
                excerpt: "Deploy the earlier change".into(),
                modified_ts: Some(10),
            },
            SessionCandidateFixture {
                session_id: "recent".into(),
                provider: "codex".into(),
                title: "Deploy command".into(),
                excerpt: "Deploy the current change".into(),
                modified_ts: Some(20),
            },
            SessionCandidateFixture {
                session_id: "miss".into(),
                provider: "claude".into(),
                title: "Deploy command".into(),
                excerpt: "Search the session index".into(),
                modified_ts: Some(30),
            },
        ];

        let result = session_additions("deploy lighthouse", &fixtures, 1);

        assert_eq!(result.len(), 1);
        assert_eq!(result[0].session_id, "recent");
    }

    #[test]
    fn decision_session_candidate_can_win_under_the_stub() {
        let session = SessionCandidateFixture {
            session_id: "sess-42".into(),
            provider: "claude".into(),
            title: "Get prior command".into(),
            excerpt: "Run codeterm sessions get sess-42 for the earlier result".into(),
            modified_ts: Some(100),
        };
        let candidate = session_candidate(&session, 2000);
        let unrelated = RerankCandidate {
            key: "memory-1".into(),
            title: "Terminal colors".into(),
            kind: "fact".into(),
            body: "The terminal background is blue.".into(),
        };

        let outcome = run_stage(
            Arc::new(StubModel),
            "codeterm sessions get",
            &[unrelated, candidate.clone()],
            &StageConfig {
                max_concurrency: 1,
                deadline: Duration::from_secs(1),
                low_confidence_threshold: 0.15,
                max_body_chars: 2000,
            },
        );

        assert_eq!(candidate.kind, "session");
        assert!(matches!(outcome, StageOutcome::Applied { order, .. } if order == vec![1, 0]));
    }
    #[test]
    fn decision_graph_additions_skip_pooled_and_cap() {
        use codeterm_lib::mem::{graph, MemKind, MemScope, NewMemItem, Relation};
        let mem = MemManager::open_in_memory().unwrap();
        let ids: Vec<String> = ["pooled one", "graph two", "graph three"]
            .iter()
            .map(|title| {
                mem.insert_raw(&NewMemItem {
                    kind: MemKind::Fact,
                    scope: MemScope::Global,
                    title: (*title).into(),
                    summary: None,
                    area: None,
                    tags: Vec::new(),
                    body: format!("{title} body"),
                    source: None,
                    importance: 3.0,
                    source_path: None,
                    pinned: false,
                })
                .unwrap()
            })
            .collect();
        mem.with_conn(|conn| {
            conn.execute(
                "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
                 VALUES ('tech_quasar', 'Tech', 'global', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
                [],
            )
            .unwrap();
            for id in &ids {
                graph::insert_relation(
                    conn,
                    &Relation {
                        from_id: id.clone(),
                        relation_type: "mentions".into(),
                        to_id: "tech_quasar".into(),
                        scope: "global".into(),
                        valid_from: "2026-01-01T00:00:00Z".into(),
                        valid_to: None,
                        confidence: 1.0,
                        properties: serde_json::json!({}),
                        reason: None,
                    },
                )
                .unwrap();
            }
        });
        let pool: Vec<SearchHit> = graph_additions(&mem, "quasar", &[], 10)
            .unwrap()
            .into_iter()
            .filter(|h| h.item.id == ids[0])
            .collect();
        assert_eq!(pool.len(), 1);
        for (m, expected) in [(1, 1), (5, 2)] {
            let added = graph_additions(&mem, "quasar", &pool, m).unwrap();
            assert_eq!(added.len(), expected, "m={m}");
            assert!(added.iter().all(|h| h.item.id != ids[0]), "m={m}");
        }
    }
    #[test]
    fn decision_negative_low_confidence_precision() {
        let metrics = negative_metrics(&[true, true, false], &[true, false]);
        assert_eq!(metrics.low_confidence_precision, 2.0 / 3.0);
        assert_eq!(metrics.positive_low_confidence_false_positive_rate, 0.5);
    }
}
