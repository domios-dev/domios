use super::*;
use crate::mem::decision::rerank::{
    run_stage, FallbackReason, RerankCandidate, StageConfig, StageOutcome,
};
use codeterm_types::decision::{DecisionPrimitives, DecisionQuestion};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

struct Model {
    mode: TokenMode,
    calls: Arc<AtomicUsize>,
}
impl DecisionModel for Model {
    fn id(&self) -> &str {
        "fixture"
    }
    fn token_mode(&self) -> TokenMode {
        self.mode
    }
    fn model_id(&self) -> Option<String> {
        Some("reported-model".into())
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            noul: true,
            ..Default::default()
        }
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        codeterm_types::decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(DecisionAnswer::Noul { p: 0.8 })
    }
}

#[test]
fn supporter_connected_predicate_by_token_mode() {
    for mode in [TokenMode::Required, TokenMode::Optional, TokenMode::None] {
        let registry = SupporterRegistry::new();
        registry.register(
            "unrelated-adapter-name",
            Arc::new(Model {
                mode,
                calls: Arc::default(),
            }),
        );
        let cfg = SupporterConfig {
            adapter: Some("unrelated-adapter-name".into()),
            ..Default::default()
        };
        for usable in [true, false] {
            assert_eq!(
                supporter_connected(
                    &cfg,
                    &registry,
                    &TokenSnapshot {
                        usable,
                        acl_invalid: false,
                    }
                ),
                mode != TokenMode::Required || usable
            );
        }
        assert!(!supporter_connected(
            &cfg,
            &SupporterRegistry::new(),
            &TokenSnapshot {
                usable: true,
                acl_invalid: false,
            }
        ));
        assert!(!supporter_connected(
            &SupporterConfig {
                adapter: None,
                ..cfg
            },
            &registry,
            &TokenSnapshot {
                usable: true,
                acl_invalid: false,
            }
        ));
    }
}

#[test]
fn supporter_invalid_token_acl_notifies_owner_once_per_invalid_period() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = SupporterRuntime::new(dir.path().into());
    let alerts = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let captured = alerts.clone();
    runtime.attach_alert_sink(Arc::new(move |alert| captured.lock().push(alert)));
    let invalid = TokenSnapshot::from_status(true, Some(false), true);

    assert!(!invalid.usable);
    assert!(invalid.acl_invalid);
    report_token_acl_state(&runtime, "typesafe-jev", &invalid);
    report_token_acl_state(&runtime, "typesafe-jev", &invalid);
    let repaired = TokenSnapshot::from_status(true, Some(true), true);
    report_token_acl_state(&runtime, "typesafe-jev", &repaired);
    report_token_acl_state(&runtime, "typesafe-jev", &invalid);

    let notices = alerts.lock();
    assert_eq!(notices.len(), 2);
    assert_eq!(
        notices[0].level,
        codeterm_notification_core::contract::NotifyLevel::Failure
    );
    assert!(!notices[0].title.contains("private-token-fixture"));
}

#[test]
fn supporter_required_adapter_rejects_corrupt_and_unusable_tokens() {
    let dir = tempfile::tempdir().unwrap();
    let store = TokenStore::new(dir.path().into());
    store.set("fixture", "private-token-fixture").unwrap();
    assert!(TokenSnapshot::read(&store, "fixture").usable);
    std::fs::write(store.path("fixture"), "invalid-json").unwrap();
    assert!(!TokenSnapshot::read(&store, "fixture").usable);
    store.set("fixture", "private-token-fixture").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(
            store.path("fixture"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        let snapshot = TokenSnapshot::read(&store, "fixture");
        assert!(!snapshot.usable);
        assert!(snapshot.acl_invalid);
    }
}

#[test]
fn supporter_budget_gate_blocks_past_cap_and_falls_back() {
    let dir = tempfile::tempdir().unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            mode: TokenMode::None,
            calls: calls.clone(),
        }),
        runtime.clone(),
        1,
        VerdictKind::Rerank,
        None,
        None,
        None,
    );
    let request = DecisionRequest {
        state: json!("screen-and-token-must-not-be-recorded"),
        instructions: "question".into(),
        question: DecisionQuestion::Noul { criteria: None },
    };
    model
        .decide(&request, Instant::now() + Duration::from_secs(1))
        .unwrap();
    let candidate = RerankCandidate {
        key: "candidate".into(),
        title: "title".into(),
        kind: "fact".into(),
        body: "body".into(),
    };
    let stage = StageConfig {
        request_mode: codeterm_types::config::RerankRequestMode::PerCandidate,
        max_concurrency: 1,
        deadline: Duration::from_secs(1),
        low_confidence_threshold: 0.15,
        max_body_chars: 100,
    };
    assert_eq!(
        run_stage(model, "query", &[candidate], &stage),
        StageOutcome::Fallback {
            reason: FallbackReason::Budget
        }
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["budget"]["reached"], true);
}

#[test]
fn supporter_journal_records_distribution_model_id_and_criteria_version() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(SupporterRuntime::new(dir.path().into()));
    let model = budgeted_model_with_activity(
        Arc::new(Model {
            mode: TokenMode::None,
            calls: Arc::default(),
        }),
        runtime,
        10,
        VerdictKind::TabState,
        Some("tab".into()),
        Some(criteria_hash(b"criteria")),
        None,
    );
    model
        .decide(
            &DecisionRequest {
                state: json!({ "screen": "private raw screen", "token": "private token" }),
                instructions: "question".into(),
                question: DecisionQuestion::Noul { criteria: None },
            },
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    let text = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    assert!(!text.contains("private raw screen"));
    assert!(!text.contains("private token"));
    let verdict: Value = text
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|line| line["action"] == "verdict")
        .unwrap();
    assert_eq!(
        verdict["distribution"]["q"],
        json!({ "type": "noul", "p": 0.8 })
    );
    assert_eq!(verdict["model_id"], "reported-model");
    assert_eq!(verdict["criteria_version"], criteria_hash(b"criteria"));
    assert_eq!(verdict["state_hash"].as_str().unwrap().len(), 64);
}
