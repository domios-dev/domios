use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::Mutex;
use serde_json::{json, Value};

use super::{decision_token_dir, token_store, DecisionRegistry};
use crate::api::mem_routes::search::{
    baseline_search, resolve_search, run_blocking_search, RecallBackend, SearchError, SearchParams,
    RERANK_EXCLUSIVE,
};
use crate::config::schema::DecisionRerankConfig;
use crate::mem::decision::{
    DecisionAnswer, DecisionError, DecisionModel, DecisionPrimitives, DecisionRequest,
};
use crate::mem::rerank::RerankConfig;
use crate::mem::search::{SearchConfig, SearchQuery};
use crate::mem::{MemKind, MemManager, MemScope, NewMemItem, SearchHit};
use crate::service::plan::ports::PlanIndexQuery;
use crate::service::plan::search_index::PlanSearchHit;

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
    plans: Vec<PlanSearchHit>,
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

fn seeded() -> SeededBackend {
    let mem = MemManager::open_in_memory().unwrap();
    for (index, title) in TITLES.iter().enumerate() {
        mem.insert_raw(&NewMemItem {
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
    }
    SeededBackend {
        mem,
        plans: vec![plan_hit()],
    }
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

fn enabled_cfg() -> DecisionRerankConfig {
    DecisionRerankConfig {
        enabled: true,
        adapter: STUB_ID.into(),
        ..DecisionRerankConfig::default()
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
}

impl DecisionModel for StubModel {
    fn id(&self) -> &str {
        STUB_ID
    }

    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..DecisionPrimitives::default()
        }
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
                    .find(|(title, _)| req.state.contains(title.as_str()))
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

fn registry_with(mode: StubMode) -> DecisionRegistry {
    let registry = DecisionRegistry::new();
    registry.register(STUB_ID, Arc::new(StubModel { mode }));
    registry
}

fn relevance_registry(by_title: &[(&str, f64)]) -> DecisionRegistry {
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
    output.as_object_mut().unwrap().remove("decision_rerank");
    output
}

#[test]
fn decision_off_response_is_byte_identical() {
    let backend = seeded();
    let registry = relevance_registry(&[(TITLES[3], 0.9)]);
    let mut request = params(3);
    request.project = Some("/seeded/project".into());
    let baseline = baseline_search(&backend, &request).unwrap();
    assert!(!baseline["hits"].as_array().unwrap().is_empty());

    let off = DecisionRerankConfig {
        adapter: STUB_ID.into(),
        ..DecisionRerankConfig::default()
    };
    let default_out = resolve_search(&backend, &registry, &off, request.clone()).unwrap();
    let overridden = SearchParams {
        decision_rerank: Some(false),
        ..request.clone()
    };
    let override_out = resolve_search(&backend, &registry, &enabled_cfg(), overridden).unwrap();

    let expected = serde_json::to_string(&baseline).unwrap();
    for output in [default_out, override_out] {
        assert_eq!(serde_json::to_string(&output).unwrap(), expected);
        assert!(output.get("decision_rerank").is_none());
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
    let pooled = baseline_search(&backend, &params(cfg.candidates)).unwrap();
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
        decision_rerank: Some(true),
        ..params(limit)
    };
    let output = resolve_search(&backend, &registry, &cfg, request).unwrap();

    let expected: Vec<String> = pool_titles.iter().rev().take(limit).cloned().collect();
    assert_eq!(titles(&output), expected);
    assert_ne!(
        titles(&output),
        titles(&baseline_search(&backend, &params(limit)).unwrap())
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
        output["decision_rerank"],
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
    let backend = seeded();
    let registry = relevance_registry(&[
        (TITLES[0], 0.02),
        (TITLES[1], 0.10),
        (TITLES[2], 0.05),
        (TITLES[3], 0.01),
    ]);
    let request = SearchParams {
        decision_rerank: Some(true),
        ..params(3)
    };
    let output = resolve_search(&backend, &registry, &enabled_cfg(), request).unwrap();

    assert_eq!(titles(&output), vec![TITLES[1], TITLES[2], TITLES[0]]);
    assert_eq!(output["decision_rerank"]["status"], "applied");
    assert_eq!(output["decision_rerank"]["low_confidence"], true);
    assert_eq!(output["decision_rerank"]["max_relevance"], 0.10);
}

#[test]
fn decision_fallback_hits_equal_baseline() {
    let (_held_sender, never_signalled) = mpsc::channel::<()>();
    let cases: Vec<(&str, DecisionRegistry, DecisionRerankConfig)> = vec![
        (
            "adapter_error",
            registry_with(StubMode::Fail(DecisionError::Http(500))),
            enabled_cfg(),
        ),
        (
            "deadline",
            registry_with(StubMode::Fail(DecisionError::Deadline)),
            enabled_cfg(),
        ),
        (
            "deadline",
            registry_with(StubMode::Gate {
                signal: Mutex::new(never_signalled),
                released: AtomicBool::new(false),
            }),
            DecisionRerankConfig {
                deadline_ms: 100,
                ..enabled_cfg()
            },
        ),
        ("no_adapter", DecisionRegistry::new(), enabled_cfg()),
        (
            "unavailable",
            registry_with(StubMode::Fail(DecisionError::Unavailable(
                "no token".into(),
            ))),
            enabled_cfg(),
        ),
    ];
    let backend = seeded();
    let mut request = params(2);
    request.project = Some("/seeded/project".into());
    let baseline = baseline_search(&backend, &request).unwrap();
    let expected_hits = serde_json::to_string(&baseline["hits"]).unwrap();

    for (reason, registry, cfg) in cases {
        let output = resolve_search(
            &backend,
            &registry,
            &cfg,
            SearchParams {
                decision_rerank: Some(true),
                ..request.clone()
            },
        )
        .unwrap();
        assert_eq!(
            serde_json::to_string(&output["hits"]).unwrap(),
            expected_hits,
            "{reason}"
        );
        assert_eq!(
            output["decision_rerank"],
            json!({ "status": "fallback", "reason": reason })
        );
        assert_eq!(without_envelope(output), baseline, "{reason}");
    }
}

#[test]
fn decision_legacy_and_decision_both_explicit_rejected() {
    let backend = seeded();
    let registry = relevance_registry(&[]);
    let request = SearchParams {
        rerank: Some(true),
        decision_rerank: Some(true),
        ..params(2)
    };
    for cfg in [DecisionRerankConfig::default(), enabled_cfg()] {
        let error = resolve_search(&backend, &registry, &cfg, request.clone()).unwrap_err();
        assert_eq!(error, SearchError::InvalidParams(RERANK_EXCLUSIVE.into()));
        assert_eq!(error.into_http().0, axum::http::StatusCode::BAD_REQUEST);
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
    let legacy = baseline_search(&backend, &request).unwrap();
    let output = resolve_search(&backend, &registry, &enabled_cfg(), request).unwrap();

    assert_eq!(
        output["decision_rerank"],
        json!({ "status": "skipped", "reason": "legacy_rerank" })
    );
    assert!(output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit.get("relevance").is_none()));
    assert_eq!(without_envelope(output), legacy);
}

#[test]
fn decision_http_search_does_not_block_runtime() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();
    let (release, signal) = mpsc::channel::<()>();
    let backend = Arc::new(seeded());
    let registry = Arc::new(registry_with(StubMode::Gate {
        signal: Mutex::new(signal),
        released: AtomicBool::new(false),
    }));
    let cfg = DecisionRerankConfig {
        deadline_ms: 10_000,
        ..enabled_cfg()
    };
    let request = SearchParams {
        decision_rerank: Some(true),
        ..params(2)
    };

    let output = runtime
        .block_on(async move {
            let search =
                run_blocking_search(move || resolve_search(&*backend, &registry, &cfg, request));
            // Runs only while the runtime thread is free, i.e. while the search is off-runtime.
            let releaser = tokio::spawn(async move { release.send(()).unwrap() });
            let output = search.await;
            releaser.await.unwrap();
            output
        })
        .unwrap();

    assert_eq!(output["decision_rerank"]["status"], "applied");
    assert_eq!(output["hits"].as_array().unwrap().len(), 2);
}

#[test]
fn decision_token_dir_is_outside_repo() {
    let dir = decision_token_dir();
    assert!(dir.starts_with(codeterm_paths::data_dir()));
    assert!(dir.ends_with("decision-models"));
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    assert!(!dir.starts_with(repo_root), "{}", dir.display());
    assert!(token_store().path(STUB_ID).starts_with(&dir));
}
