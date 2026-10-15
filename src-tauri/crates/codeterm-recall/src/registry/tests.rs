use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use codeterm_decision::{
    deadline, DecisionAnswer, DecisionError, DecisionModel, DecisionModelInfo, DecisionPrimitives,
    DecisionRequest, TokenMode,
};
use parking_lot::Mutex;
use serde_json::{json, Value};

use super::sessions_source::SessionCandidate;
use super::{
    candidate, registered_adapters, supporter_state, supporter_token_dir, token_store,
    DecisionBackend, SupporterRegistry, Untouched,
};
use crate::search::{
    baseline_search, resolve_search, RecallBackend, SearchError, SearchParams, RERANK_EXCLUSIVE,
};
use codeterm_mem::decision::token_store::TokenStatus;
use codeterm_mem::rerank::RerankConfig;
use codeterm_mem::search::{SearchConfig, SearchQuery};
use codeterm_mem::{MemKind, MemManager, MemScope, NewMemItem, SearchHit};
use codeterm_plan::service_plan::ports::PlanIndexQuery;
use codeterm_plan::service_plan::search_index::PlanSearchHit;
use codeterm_types::config::{MemoryRerankConfig, SupporterConfig};

const STUB_ID: &str = "stub-decision";
const QUERY: &str = "lighthouse";
const TITLES: [&str; 4] = [
    "lighthouse alpha",
    "lighthouse bravo",
    "lighthouse charlie",
    "lighthouse delta",
];

struct SeededBackend {
    mem: Arc<MemManager>,
    ids: Vec<String>,
    plans: Vec<PlanSearchHit>,
    graph_calls: AtomicUsize,
    sessions: Vec<SessionCandidate>,
    runtime: Arc<codeterm_supporter::SupporterRuntime>,
    _journal_dir: tempfile::TempDir,
}

impl RecallBackend for SeededBackend {
    fn recall(&self, q: &SearchQuery, cfg: &SearchConfig) -> Result<Vec<SearchHit>, String> {
        self.mem.recall_with_config(q, cfg)
    }

    /// Deterministic stand-in: the real legacy reranker may load a local model or call an API.
    fn legacy_rerank(
        &self,
        q: &SearchQuery,
        _cfg: &RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let mut hits = self.mem.recall_with_config(q, &SearchConfig::default())?;
        hits.reverse();
        Ok(hits)
    }

    fn plan_hits(&self, _project: &str, q: &PlanIndexQuery) -> Result<Vec<PlanSearchHit>, String> {
        let limit = q.limit.unwrap_or(usize::MAX);
        Ok(self.plans.iter().take(limit).cloned().collect())
    }
}

impl DecisionBackend for SeededBackend {
    fn supporter_runtime(&self) -> Arc<codeterm_supporter::SupporterRuntime> {
        self.runtime.clone()
    }
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
        _cfg: &RerankConfig,
    ) -> Result<Vec<SearchHit>, String> {
        let mut hits = self
            .mem
            .recall_with_config_untouched(q, &SearchConfig::default())?;
        hits.reverse();
        Ok(hits)
    }

    fn graph_candidates_untouched(
        &self,
        q: &SearchQuery,
        m: usize,
    ) -> Result<Vec<SearchHit>, String> {
        self.graph_calls.fetch_add(1, Ordering::SeqCst);
        self.mem.graph_candidates_untouched(q, m)
    }

    fn session_candidates(
        &self,
        _query: &str,
        _limit: usize,
        _max_body_chars: usize,
        _deadline: Instant,
    ) -> Vec<SessionCandidate> {
        self.sessions.clone()
    }

    fn record_access(&self, ids: &[String]) -> Result<(), String> {
        self.mem.record_access(ids)
    }
}

fn seeded() -> SeededBackend {
    let mem = MemManager::open_in_memory().unwrap();
    let mut ids = Vec::new();
    for (index, title) in TITLES.iter().enumerate() {
        let id = mem
            .insert_raw(&NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: (*title).to_string(),
                summary: None,
                area: None,
                tags: Vec::new(),
                body: format!("{QUERY} keeper log entry number {index}"),
                source: None,
                importance: 0.5,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        ids.push(id);
    }
    let journal_dir = tempfile::tempdir().unwrap();
    SeededBackend {
        runtime: Arc::new(codeterm_supporter::SupporterRuntime::new(
            journal_dir.path().into(),
        )),
        _journal_dir: journal_dir,
        mem,
        ids,
        plans: vec![plan_hit()],
        graph_calls: AtomicUsize::new(0),
        sessions: Vec::new(),
    }
}

/// What the baseline returns for `p` on the store's current state, without mutating it.
fn baseline_oracle(backend: &SeededBackend, p: &SearchParams) -> Value {
    baseline_search(&Untouched(backend), p).unwrap()
}

/// `accessed_at` is the wall-clock time of the read, the one field two reads never share.
fn normalized(mut output: Value) -> String {
    if let Some(hits) = output["hits"].as_array_mut() {
        for hit in hits {
            if let Some(stamp) = hit.pointer_mut("/item/accessed_at") {
                if !stamp.is_null() {
                    *stamp = json!("<accessed_at>");
                }
            }
        }
    }
    serde_json::to_string(&output).unwrap()
}

fn access_counts(backend: &SeededBackend) -> HashMap<String, i64> {
    backend
        .ids
        .iter()
        .map(|id| (id.clone(), backend.mem.access_count(id).unwrap().unwrap()))
        .collect()
}

fn returned_mem_ids(output: &Value) -> HashSet<String> {
    output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|hit| hit["source"] == "mem")
        .map(|hit| hit["item"]["id"].as_str().unwrap().to_string())
        .collect()
}

fn plan_hit() -> PlanSearchHit {
    PlanSearchHit {
        source: "plan",
        score: 0.5,
        id: "p1".into(),
        kind: "task".into(),
        title: "lighthouse echo plan".into(),
        body: "plan lighthouse task".into(),
        scope: "plan_scope".into(),
        updated_at: 1234,
        age_ms: 5678,
        plan_id: Some("pl_1".into()),
        spec_id: None,
        unit_id: "task_1".into(),
        unit_title: "Task".into(),
        parent_title: "Plan".into(),
        status: Some("pending".into()),
    }
}

fn params(limit: usize) -> SearchParams {
    SearchParams {
        q: QUERY.into(),
        limit: Some(limit),
        ..SearchParams::default()
    }
}

/// Recall-only pool: the shipped defaults add graph candidates, which these tests do not cover.
fn enabled_cfg() -> SupporterConfig {
    SupporterConfig {
        adapter: Some(STUB_ID.into()),
        memory_rerank: MemoryRerankConfig {
            sources: codeterm_types::config::SupporterSources {
                graph: 0,
                docs: 0,
                ..codeterm_types::config::SupporterSources::default()
            },
            ..MemoryRerankConfig::default()
        },
        ..SupporterConfig::default()
    }
}

enum StubMode {
    Relevance(HashMap<String, f64>),
    Fail(DecisionError),
    Gate {
        signal: Mutex<mpsc::Receiver<()>>,
        released: AtomicBool,
    },
}

struct StubModel {
    mode: StubMode,
    finished: Option<mpsc::Sender<()>>,
}

impl Drop for StubModel {
    fn drop(&mut self) {
        if let Some(finished) = &self.finished {
            let _ = finished.send(());
        }
    }
}

impl DecisionModel for StubModel {
    fn supports_question_batch(&self) -> bool {
        true
    }

    fn decide_questions(
        &self,
        request: &codeterm_decision::DecisionQuestionsRequest,
        deadline: Instant,
    ) -> Result<codeterm_decision::DecisionResponse, DecisionError> {
        codeterm_decision::physical::before_request()?;
        let answers = request
            .questions
            .iter()
            .map(|(name, question)| {
                let candidate = &request.state["candidates"][name];
                let single = DecisionRequest {
                    state: Value::String(format!(
                        "{}\n{}",
                        candidate["title"].as_str().unwrap_or_default(),
                        candidate["body"].as_str().unwrap_or_default()
                    )),
                    instructions: question.instructions.clone(),
                    question: question.question.clone(),
                };
                self.decide(&single, deadline)
                    .map(|answer| (name.clone(), answer))
            })
            .collect::<Result<_, _>>()?;
        Ok(codeterm_decision::DecisionResponse {
            answers,
            usage: None,
        })
    }

    fn id(&self) -> &str {
        STUB_ID
    }

    fn token_mode(&self) -> TokenMode {
        TokenMode::None
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..DecisionPrimitives::default()
        }
    }

    fn model_id(&self) -> Option<String> {
        Some("stub-model".into())
    }

    fn models(&self) -> Vec<DecisionModelInfo> {
        vec![DecisionModelInfo {
            id: "stub-model".into(),
            display_name: "Stub Model".into(),
        }]
    }

    fn decide(
        &self,
        req: &DecisionRequest,
        deadline: Instant,
    ) -> Result<DecisionAnswer, DecisionError> {
        match &self.mode {
            StubMode::Relevance(by_title) => {
                let p = by_title
                    .iter()
                    .find(|(title, _)| {
                        req.state
                            .as_str()
                            .unwrap_or_default()
                            .contains(title.as_str())
                    })
                    .map_or(0.0, |(_, p)| *p);
                Ok(DecisionAnswer::Noul { p })
            }
            StubMode::Fail(error) => Err(error.clone()),
            StubMode::Gate { signal, released } => {
                let signal = signal.lock();
                if !released.load(Ordering::SeqCst) {
                    signal
                        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                        .map_err(|_| DecisionError::Deadline)?;
                    released.store(true, Ordering::SeqCst);
                }
                Ok(DecisionAnswer::Noul { p: 0.5 })
            }
        }
    }
}

fn registry_with(mode: StubMode) -> SupporterRegistry {
    let registry = SupporterRegistry::new();
    registry.register(
        STUB_ID,
        Arc::new(StubModel {
            mode,
            finished: None,
        }),
    );
    registry
}

fn registry_with_completion(mode: StubMode) -> (SupporterRegistry, mpsc::Receiver<()>) {
    let (finished, completion) = mpsc::channel();
    let registry = SupporterRegistry::new();
    registry.register(
        STUB_ID,
        Arc::new(StubModel {
            mode,
            finished: Some(finished),
        }),
    );
    (registry, completion)
}

fn finish_registry(registry: SupporterRegistry, completion: mpsc::Receiver<()>) {
    drop(registry);
    completion
        .recv_timeout(Duration::from_secs(10))
        .expect("fixture_adapter_cleanup_liveness_exceeded");
}

fn relevance_registry(by_title: &[(&str, f64)]) -> SupporterRegistry {
    registry_with(StubMode::Relevance(
        by_title.iter().map(|(t, p)| (t.to_string(), *p)).collect(),
    ))
}

fn hit_title(hit: &Value) -> String {
    hit.get("item")
        .unwrap_or(hit)
        .get("title")
        .and_then(Value::as_str)
        .unwrap()
        .to_string()
}

fn titles(output: &Value) -> Vec<String> {
    output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .map(hit_title)
        .collect()
}

fn without_envelope(mut output: Value) -> Value {
    output.as_object_mut().unwrap().remove("supporter");
    output
}

#[test]
fn supporter_status_distinguishes_not_connected_token_states_and_active() {
    let missing = TokenStatus {
        present: false,
        path: "token.json".into(),
        mode_ok: None,
    };
    let set = TokenStatus {
        present: true,
        path: "token.json".into(),
        mode_ok: Some(true),
    };
    let unusable = TokenStatus {
        present: true,
        path: "token.json".into(),
        mode_ok: Some(false),
    };

    assert_eq!(
        supporter_state(true, &missing, true, TokenMode::Required),
        "no_token"
    );
    assert_eq!(
        supporter_state(true, &set, true, TokenMode::Required),
        "active"
    );
    assert_eq!(
        supporter_state(true, &set, true, TokenMode::Required),
        "active"
    );
    assert_eq!(
        supporter_state(true, &unusable, true, TokenMode::Required),
        "token_unusable"
    );
    assert_eq!(
        supporter_state(true, &set, false, TokenMode::Required),
        "adapter_unavailable"
    );
}

#[test]
fn supporter_state_optional_and_none_ignore_missing_tokens() {
    let missing = TokenStatus {
        present: false,
        path: "token.json".into(),
        mode_ok: None,
    };
    for mode in [TokenMode::Optional, TokenMode::None] {
        assert_eq!(
            supporter_state(false, &missing, true, mode),
            "not_connected"
        );
        assert_eq!(supporter_state(true, &missing, true, mode), "active");
        assert_eq!(
            supporter_state(true, &missing, false, mode),
            "adapter_unavailable"
        );
    }
}

#[test]
fn supporter_models_lists_registered_models() {
    let registry = registry_with(StubMode::Relevance(HashMap::new()));

    assert_eq!(
        registered_adapters(&registry),
        vec![json!({
            "id": STUB_ID,
            "token_mode": "none",
            "pricing": null,
            "metadata": { "display_name": null, "help_url": null, "server_address": null, "settings_url": null },
            "primitives": { "choice": false, "noul": true, "score": false },
            "models": [{ "id": "stub-model", "display_name": "Stub Model" }],
        })]
    );
}

#[test]
fn decision_candidate_carries_kind() {
    let mem_hit = json!({
        "item": {"id": "mem-1", "title": "Memory title", "kind": "fact", "body": "Memory body"}
    });
    let plan_hit = json!({
        "id": "plan-1", "title": "Plan title", "kind": "task", "body": "Plan body"
    });
    let missing_kind_hit = json!({"id": "other", "title": "Other title", "body": "Other body"});

    assert_eq!(candidate(0, &mem_hit).kind, "fact");
    assert_eq!(candidate(1, &plan_hit).kind, "task");
    assert_eq!(candidate(2, &missing_kind_hit).kind, "");
}

#[test]
fn supporter_not_connected_search_is_baseline_identical() {
    let backend = seeded();
    let registry = relevance_registry(&[(TITLES[3], 0.9)]);
    let mut request = params(3);
    request.project = Some("/seeded/project".into());
    let off = SupporterConfig {
        adapter: Some("unregistered".into()),
        ..SupporterConfig::default()
    };
    let overridden = SearchParams {
        supporter: Some(false),
        ..request.clone()
    };
    for (cfg, request) in [(off, request.clone()), (enabled_cfg(), overridden)] {
        let expected = baseline_oracle(&backend, &request);
        assert!(!expected["hits"].as_array().unwrap().is_empty());
        let before = access_counts(&backend);
        let output = resolve_search(&backend, &registry, &cfg, request).unwrap();
        let after = access_counts(&backend);

        assert_eq!(normalized(output.clone()), normalized(expected));
        // Off is the unchanged baseline path, so it still records access for every item it hydrates.
        assert!(returned_mem_ids(&output)
            .iter()
            .all(|id| after[id] > before[id]));
        assert!(output.get("supporter").is_none());
        assert!(output["hits"]
            .as_array()
            .unwrap()
            .iter()
            .all(|hit| hit.get("relevance").is_none()));
    }
}

#[test]
fn decision_applied_reorders_and_annotates() {
    let backend = seeded();
    let cfg = enabled_cfg();
    let pooled = baseline_oracle(&backend, &params(cfg.memory_rerank.sources.recall));
    let pool_titles = titles(&pooled);
    assert_eq!(pool_titles.len(), TITLES.len());
    let n = pool_titles.len() as f64;
    let by_title: Vec<(&str, f64)> = pool_titles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), (i as f64 + 1.0) / (n + 1.0)))
        .collect();
    let registry = relevance_registry(&by_title);

    let limit = 2;
    let request = SearchParams {
        supporter: Some(true),
        ..params(limit)
    };
    let output = resolve_search(&backend, &registry, &cfg, request).unwrap();

    let expected: Vec<String> = pool_titles.iter().rev().take(limit).cloned().collect();
    assert_eq!(titles(&output), expected);
    assert_ne!(
        titles(&output),
        titles(&baseline_oracle(&backend, &params(limit)))
    );
    for hit in output["hits"].as_array().unwrap() {
        let wanted = by_title
            .iter()
            .find(|(t, _)| *t == hit_title(hit))
            .unwrap()
            .1;
        assert_eq!(hit["relevance"].as_f64().unwrap(), wanted);
    }
    assert_eq!(
        output["supporter"],
        json!({
            "status": "applied",
            "adapter": STUB_ID,
            "low_confidence": false,
            "max_relevance": n / (n + 1.0),
        })
    );
}

#[test]
fn decision_applied_low_confidence_envelope() {
    let _scope = clock::fixture_clock().enter();
    let backend = seeded();
    let registry = relevance_registry(&[
        (TITLES[0], 0.02),
        (TITLES[1], 0.10),
        (TITLES[2], 0.05),
        (TITLES[3], 0.01),
    ]);
    let request = SearchParams {
        supporter: Some(true),
        ..params(3)
    };
    let output = resolve_search(&backend, &registry, &enabled_cfg(), request).unwrap();

    assert_eq!(output["supporter"]["status"], "applied", "{output}");
    assert_eq!(titles(&output), vec![TITLES[1], TITLES[2], TITLES[0]]);
    assert_eq!(output["supporter"]["low_confidence"], true);
    assert_eq!(output["supporter"]["max_relevance"], 0.10);
}

#[test]
fn decision_fallback_hits_equal_baseline() {
    let _scope = clock::fixture_clock().enter();
    let cases = [
        ("adapter_error", DecisionError::Http(500)),
        ("deadline", DecisionError::Deadline),
        ("budget", DecisionError::Budget),
        ("unavailable", DecisionError::Unavailable("no token".into())),
    ];
    let backend = seeded();
    let mut request = params(2);
    request.project = Some("/seeded/project".into());
    for (reason, error) in cases {
        let (registry, completion) = registry_with_completion(StubMode::Fail(error));
        let baseline = baseline_oracle(&backend, &request);
        let output = resolve_search(
            &backend,
            &registry,
            &enabled_cfg(),
            SearchParams {
                supporter: Some(true),
                ..request.clone()
            },
        )
        .unwrap();
        finish_registry(registry, completion);
        assert_eq!(
            normalized(json!({ "hits": output["hits"] })),
            normalized(json!({ "hits": baseline["hits"] })),
            "{reason}"
        );
        assert_eq!(
            output["supporter"],
            json!({ "status": "fallback", "reason": reason })
        );
        assert_eq!(
            normalized(without_envelope(output)),
            normalized(baseline),
            "{reason}"
        );
    }
}

#[test]
fn decision_legacy_and_decision_both_explicit_rejected() {
    let backend = seeded();
    let registry = relevance_registry(&[]);
    let request = SearchParams {
        rerank: Some(true),
        supporter: Some(true),
        ..params(2)
    };
    for cfg in [SupporterConfig::default(), enabled_cfg()] {
        let error = resolve_search(&backend, &registry, &cfg, request.clone()).unwrap_err();
        assert_eq!(error, SearchError::InvalidParams(RERANK_EXCLUSIVE.into()));
    }
}

#[test]
fn decision_legacy_with_config_enabled_reports_skipped() {
    let backend = seeded();
    let registry = relevance_registry(&[(TITLES[0], 0.9)]);
    let request = SearchParams {
        rerank: Some(true),
        ..params(3)
    };
    let legacy = baseline_oracle(&backend, &request);
    let output = resolve_search(&backend, &registry, &enabled_cfg(), request).unwrap();

    assert_eq!(
        output["supporter"],
        json!({ "status": "skipped", "reason": "legacy_rerank" })
    );
    assert!(output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit.get("relevance").is_none()));
    assert_eq!(normalized(without_envelope(output)), normalized(legacy));
}

#[test]
fn decision_access_delta_matches_returned_hits() {
    let backend = seeded();
    let mut request = params(2);
    request.project = Some("/seeded/project".into());
    let pool_titles = titles(&baseline_oracle(&backend, &params(10)));
    let by_title: Vec<(&str, f64)> = pool_titles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), (i as f64 + 1.0) / 10.0))
        .collect();
    let cases: Vec<(&str, SupporterRegistry, SearchParams)> = vec![
        (
            "applied",
            relevance_registry(&by_title),
            SearchParams {
                supporter: Some(true),
                ..request.clone()
            },
        ),
        (
            "fallback",
            registry_with(StubMode::Fail(DecisionError::Http(500))),
            SearchParams {
                supporter: Some(true),
                ..request.clone()
            },
        ),
        (
            "skipped",
            relevance_registry(&by_title),
            SearchParams {
                rerank: Some(true),
                ..request.clone()
            },
        ),
    ];

    for (status, registry, request) in cases {
        let before = access_counts(&backend);
        let output = resolve_search(&backend, &registry, &enabled_cfg(), request).unwrap();
        let after = access_counts(&backend);
        assert_eq!(output["supporter"]["status"], status);

        let returned = returned_mem_ids(&output);
        assert!(!returned.is_empty(), "{status}");
        assert!(
            returned.len() < backend.ids.len(),
            "{status}: no pool-only item"
        );
        for id in &backend.ids {
            let expected = i64::from(returned.contains(id));
            assert_eq!(after[id] - before[id], expected, "{status}: {id}");
        }
    }
}

#[test]
fn supporter_token_dir_is_under_data_dir() {
    let dir = supporter_token_dir();
    assert!(dir.starts_with(codeterm_paths::data_dir()));
    assert!(dir.ends_with("supporter"));
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(3)
        .unwrap();
    assert!(!dir.starts_with(repo_root), "{}", dir.display());
    assert!(token_store().path(STUB_ID).starts_with(&dir));
}

mod clock;
mod docs;
mod graph;
mod sessions;

mod batching;
