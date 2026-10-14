use super::*;
use codeterm_decision::{
    deadline, DecisionModelInfo, DecisionPrimitives, DecisionQuestion, DecisionRequest, TokenMode,
};
use codeterm_types::config::SupporterConnection;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Model {
    id: &'static str,
    selected: parking_lot::Mutex<String>,
    mode: TokenMode,
    choice: bool,
    calls: AtomicUsize,
}

impl Model {
    fn new(id: &'static str, selected: &str, mode: TokenMode, choice: bool) -> Arc<Self> {
        Arc::new(Self {
            id,
            selected: parking_lot::Mutex::new(selected.into()),
            mode,
            choice,
            calls: AtomicUsize::new(0),
        })
    }
}

impl DecisionModel for Model {
    fn id(&self) -> &str {
        self.id
    }
    fn token_mode(&self) -> TokenMode {
        self.mode
    }
    fn model_id(&self) -> Option<String> {
        Some(self.selected.lock().clone())
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives {
            choice: self.choice,
            ..Default::default()
        }
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        ["first", "second"]
            .into_iter()
            .map(|id| DecisionModelInfo {
                id: id.into(),
                display_name: id.into(),
            })
            .collect()
    }
    fn decide(&self, _: &DecisionRequest, _: Instant) -> Result<DecisionAnswer, DecisionError> {
        codeterm_decision::physical::before_request()?;
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(DecisionAnswer::Choice {
            choice: "accepted".into(),
            confidence: Some(1.0),
            probabilities: BTreeMap::from([("accepted".into(), 1.0)]),
        })
    }
}

fn request() -> DecisionRequest {
    DecisionRequest {
        state: json!("opaque-private-input"),
        instructions: "opaque-instructions".into(),
        question: DecisionQuestion::Choice {
            options: BTreeMap::from([("accepted".into(), "opaque-option".into())]),
        },
    }
}

fn config() -> SupporterConfig {
    SupporterConfig {
        adapter: Some("active".into()),
        connections: BTreeMap::from([
            (
                "active".into(),
                SupporterConnection {
                    model: Some("second".into()),
                    ..Default::default()
                },
            ),
            (
                "saved".into(),
                SupporterConnection {
                    model: Some("first".into()),
                    ..Default::default()
                },
            ),
        ]),
        calls_per_hour: Some(1),
        ..Default::default()
    }
}

#[test]
fn heartbeat_connections_use_only_active_saved_model_and_never_choose_another_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(supporter::SupporterRuntime::new(dir.path().into()));
    let registry = SupporterRegistry::new();
    let active = Model::new("active", "second", TokenMode::None, true);
    let saved = Model::new("saved", "first", TokenMode::None, true);
    registry.register("active", active.clone());
    registry.register("saved", saved.clone());
    let mut cfg = config();
    let token = supporter::TokenSnapshot::default();
    let model = connected_model(&cfg, &registry, &token, runtime.clone(), "owner").unwrap();
    assert_eq!(model.id(), "active");
    assert_eq!(model.model_id().as_deref(), Some("second"));
    let before = Supporter {
        epoch: runtime.connection_epoch(),
        cfg: config(),
        token: supporter::TokenSnapshot::default(),
        registry: &registry,
        runtime: runtime.clone(),
    };
    registry.unregister("active");
    runtime.connection_changed();
    registry.register("active", active.clone());
    runtime.connection_changed();
    let after = Supporter {
        epoch: runtime.connection_epoch(),
        cfg: config(),
        token: supporter::TokenSnapshot::default(),
        registry: &registry,
        runtime: runtime.clone(),
    };
    assert!(before.generation().is_some());
    assert_eq!(before.generation(), after.generation());
    assert_eq!(after.epoch, before.epoch + 2);
    cfg.adapter = Some("saved".into());
    let model = connected_model(&cfg, &registry, &token, runtime.clone(), "owner").unwrap();
    assert_eq!(model.model_id().as_deref(), Some("first"));
    assert_eq!(active.model_id().as_deref(), Some("second"));
    cfg.adapter = None;
    assert!(connected_model(&cfg, &registry, &token, runtime.clone(), "owner").is_none());
    cfg.adapter = Some("missing".into());
    assert!(connected_model(&cfg, &registry, &token, runtime.clone(), "owner").is_none());
    cfg.adapter = Some("active".into());
    *active.selected.lock() = "first".into();
    assert!(connected_model(&cfg, &registry, &token, runtime, "owner").is_none());
    assert_eq!(saved.calls.load(Ordering::SeqCst), 0);
}

#[test]
fn heartbeat_connections_respect_token_mode_and_choice_capability() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(supporter::SupporterRuntime::new(dir.path().into()));
    let cfg = config();
    for mode in [TokenMode::Required, TokenMode::Optional, TokenMode::None] {
        let registry = SupporterRegistry::new();
        registry.register("active", Model::new("active", "second", mode, true));
        for usable in [false, true] {
            let model = connected_model(
                &cfg,
                &registry,
                &supporter::TokenSnapshot {
                    usable,
                    acl_invalid: false,
                },
                runtime.clone(),
                "owner",
            );
            assert_eq!(model.is_some(), mode != TokenMode::Required || usable);
        }
        registry.register("active", Model::new("active", "second", mode, false));
        assert!(connected_model(
            &cfg,
            &registry,
            &supporter::TokenSnapshot {
                usable: true,
                acl_invalid: false,
            },
            runtime.clone(),
            "owner"
        )
        .is_none());
    }
}

#[test]
fn heartbeat_connections_share_budget_and_journal_the_selected_model() {
    let clock = deadline::FixtureClock::new(Instant::now() - Duration::from_secs(60));
    let _scope = clock.enter();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(supporter::SupporterRuntime::new(dir.path().into()));
    let registry = SupporterRegistry::new();
    let active = Model::new("active", "second", TokenMode::None, true);
    registry.register("active", active.clone());
    let cfg = config();
    let model = connected_model(
        &cfg,
        &registry,
        &supporter::TokenSnapshot::default(),
        runtime.clone(),
        "owner",
    )
    .unwrap();
    let request = request();
    model
        .decide(&request, deadline::now() + Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        model
            .decide(&request, deadline::now() + Duration::from_secs(1))
            .unwrap_err(),
        DecisionError::Budget
    );
    assert_eq!(active.calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    let journal = std::fs::read_to_string(dir.path().join("journal.jsonl")).unwrap();
    let verdict: Value = journal
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|row| row["kind"] == "heartbeat" && row["action"] == "verdict")
        .unwrap();
    assert_eq!(verdict["model_id"], "second");
    assert!(!journal.contains("opaque-private-input"));
}

#[test]
fn heartbeat_real_expired_deadline_rejects_before_physical_call() {
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(supporter::SupporterRuntime::new(dir.path().into()));
    let registry = SupporterRegistry::new();
    let active = Model::new("active", "second", TokenMode::None, true);
    registry.register("active", active.clone());
    let model = connected_model(
        &config(),
        &registry,
        &supporter::TokenSnapshot::default(),
        runtime.clone(),
        "owner",
    )
    .unwrap();
    let request = request();
    assert_eq!(
        model.decide(&request, Instant::now()).unwrap_err(),
        DecisionError::Deadline,
    );
    assert_eq!(active.calls.load(Ordering::SeqCst), 0);
    assert_eq!(runtime.status(1)["calls_last_hour"], 0);
    assert!(codeterm_decision::physical::current().is_none());
    assert!(!dir.path().join("journal.jsonl").exists());
}

#[test]
fn heartbeat_fixture_clock_expiry_stays_distinct_from_shared_budget() {
    let clock = deadline::FixtureClock::new(Instant::now() - Duration::from_secs(60));
    let _scope = clock.enter();
    let dir = tempfile::tempdir().unwrap();
    let runtime = Arc::new(supporter::SupporterRuntime::new(dir.path().into()));
    let registry = SupporterRegistry::new();
    let active = Model::new("active", "second", TokenMode::None, true);
    registry.register("active", active.clone());
    let model = connected_model(
        &config(),
        &registry,
        &supporter::TokenSnapshot::default(),
        runtime.clone(),
        "owner",
    )
    .unwrap();
    let request = request();
    assert_eq!(
        model.decide(&request, clock.now()).unwrap_err(),
        DecisionError::Deadline,
    );
    assert_eq!(active.calls.load(Ordering::SeqCst), 0);
    let limit = clock.now() + Duration::from_secs(1);
    model.decide(&request, limit).unwrap();
    clock.advance_to(limit);
    assert_eq!(
        model.decide(&request, limit).unwrap_err(),
        DecisionError::Deadline,
    );
    assert_eq!(
        model
            .decide(&request, clock.now() + Duration::from_secs(1))
            .unwrap_err(),
        DecisionError::Budget,
    );
    assert_eq!(active.calls.load(Ordering::SeqCst), 1);
    assert_eq!(runtime.status(1)["calls_last_hour"], 1);
    assert!(codeterm_decision::physical::current().is_none());
}
