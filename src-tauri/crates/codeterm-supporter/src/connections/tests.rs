use super::*;
use codeterm_decision::{
    DecisionAnswer, DecisionError, DecisionModelInfo, DecisionPrimitives, DecisionRequest,
};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Instant;

struct Model {
    id: String,
    mode: TokenMode,
    selected: parking_lot::Mutex<String>,
    unavailable: AtomicBool,
}
impl Model {
    fn new(id: &str, mode: TokenMode) -> Arc<Self> {
        Arc::new(Self {
            id: id.into(),
            mode,
            selected: parking_lot::Mutex::new("one".into()),
            unavailable: AtomicBool::new(false),
        })
    }
}
impl DecisionModel for Model {
    fn id(&self) -> &str {
        &self.id
    }
    fn token_mode(&self) -> TokenMode {
        self.mode
    }
    fn model_id(&self) -> Option<String> {
        Some(self.selected.lock().clone())
    }
    fn primitives(&self) -> DecisionPrimitives {
        DecisionPrimitives::default()
    }
    fn models(&self) -> Vec<DecisionModelInfo> {
        ["one", "two"]
            .into_iter()
            .map(|id| DecisionModelInfo {
                id: id.into(),
                display_name: id.into(),
            })
            .collect()
    }
    fn select_model(&self, id: &str) -> bool {
        if self.unavailable.load(Ordering::SeqCst)
            || !self.models().iter().any(|model| model.id == id)
        {
            return false;
        }
        *self.selected.lock() = id.into();
        true
    }
    fn decide(
        &self,
        _: &DecisionRequest,
        _: Instant,
    ) -> std::result::Result<DecisionAnswer, DecisionError> {
        Err(DecisionError::Unsupported)
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    cfg: SupporterConfig,
    store: TokenStore,
    registry: SupporterRegistry,
    a: Arc<Model>,
    b: Arc<Model>,
}
impl Fixture {
    fn new(mode: TokenMode) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let store = TokenStore::new(dir.path().into());
        let registry = SupporterRegistry::new();
        let a = Model::new("a", TokenMode::None);
        let b = Model::new("b", mode);
        registry.register("a", a.clone());
        registry.register("b", b.clone());
        Self {
            dir,
            cfg: SupporterConfig::default(),
            store,
            registry,
            a,
            b,
        }
    }
    fn service(&mut self) -> Connections<'_, impl FnMut(&SupporterConfig) -> Result<()>> {
        let path = self.dir.path().join("config.toml");
        Connections {
            cfg: &mut self.cfg,
            registry: &self.registry,
            store: &self.store,
            persist: move |cfg: &SupporterConfig| {
                std::fs::write(&path, toml::to_string(cfg).unwrap())
                    .map_err(|_| ConnectionError::unavailable())
            },
        }
    }
    fn connect_a(&mut self) {
        self.service().connect("a", None, None, true).unwrap();
    }
}

#[test]
fn supporter_connections_connect_and_activate_by_token_mode() {
    for mode in [TokenMode::Required, TokenMode::Optional, TokenMode::None] {
        let mut f = Fixture::new(mode);
        if mode == TokenMode::Required {
            assert_eq!(
                f.service().connect("b", None, None, true).unwrap_err().code,
                "authentication_failed"
            );
            assert_eq!(f.cfg.adapter, None);
        }
        let secret = (mode == TokenMode::Required).then_some("opaque-private-token");
        let result = f.service().connect("b", None, secret, true).unwrap();
        assert_eq!(result["state"], "active");
        assert_eq!(f.cfg.active_id(), Some("b"));
        assert_eq!(f.store.status("b").unwrap().present, secret.is_some());
    }
}

#[test]
fn supporter_connections_partial_failure_saved_not_active() {
    let mut f = Fixture::new(TokenMode::Required);
    f.connect_a();
    f.b.unavailable.store(true, Ordering::SeqCst);
    let result = f
        .service()
        .connect("b", Some("two".into()), Some("private-token"), true)
        .unwrap();
    assert_eq!(result["state"], "saved_not_active");
    assert_eq!(result["error"]["code"], "unavailable");
    assert_eq!(f.cfg.active_id(), Some("a"));
    assert!(f.store.status("b").unwrap().present);
    assert_eq!(f.cfg.connection("b").model.as_deref(), Some("two"));
    f.b.unavailable.store(false, Ordering::SeqCst);
    assert_eq!(f.service().activate("b").unwrap()["state"], "active");
}

#[test]
fn supporter_connections_activate_without_token_prompt_keeps_old_on_failure() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", None, Some("private-token"), true)
        .unwrap();
    f.connect_a();
    f.b.unavailable.store(true, Ordering::SeqCst);
    assert_eq!(f.service().activate("b").unwrap_err().code, "unavailable");
    assert_eq!(f.cfg.active_id(), Some("a"));
    f.b.unavailable.store(false, Ordering::SeqCst);
    f.service().activate("b").unwrap();
    assert_eq!(f.cfg.active_id(), Some("b"));
    assert!(f.store.status("b").unwrap().present);
}

#[test]
fn supporter_connections_set_model_never_activates() {
    let mut f = Fixture::new(TokenMode::Optional);
    f.service().connect("b", None, None, true).unwrap();
    f.connect_a();
    f.service().set_model("b", "two".into()).unwrap();
    assert_eq!(f.cfg.active_id(), Some("a"));
    assert_eq!(f.cfg.connection("b").model.as_deref(), Some("two"));
}

#[test]
fn supporter_connections_model_persists_per_adapter_across_restart() {
    let mut f = Fixture::new(TokenMode::None);
    f.connect_a();
    f.service()
        .connect("b", Some("two".into()), None, true)
        .unwrap();
    let cfg: SupporterConfig =
        toml::from_str(&std::fs::read_to_string(f.dir.path().join("config.toml")).unwrap())
            .unwrap();
    let restored = cfg.clone();
    let registry = SupporterRegistry::with_registration_hook(move |id, adapter| {
        crate::restore_model(&restored, id, adapter);
    });
    registry.register("a", Model::new("a", TokenMode::None));
    registry.register("b", Model::new("b", TokenMode::None));
    assert_eq!(
        crate::configured_model(&cfg, &registry)
            .unwrap()
            .model_id()
            .as_deref(),
        Some("two")
    );
    assert_eq!(cfg.connection("a").model.as_deref(), Some("one"));
    assert_eq!(
        registry.get("a").unwrap().model_id().as_deref(),
        Some("one")
    );
}

#[test]
fn supporter_connections_model_change_reaches_running_adapter_without_leak() {
    let mut f = Fixture::new(TokenMode::None);
    f.connect_a();
    f.service().connect("b", None, None, true).unwrap();
    f.service().set_model("b", "two".into()).unwrap();
    assert_eq!(f.b.model_id().as_deref(), Some("two"));
    assert_eq!(f.a.model_id().as_deref(), Some("one"));
    assert_eq!(f.cfg.connection("a").model.as_deref(), Some("one"));
}

#[test]
fn supporter_connections_replace_token_keeps_model_and_activation() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", Some("two".into()), Some("first-secret"), true)
        .unwrap();
    let old = f.cfg.clone();
    f.service().replace_token("b", "second-secret").unwrap();
    assert_eq!(f.cfg, old);
    assert_eq!(
        f.store.load("b").unwrap().unwrap().expose(),
        "second-secret"
    );
}

#[test]
fn supporter_connections_disconnect_inactive_removes_token_keeps_active() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", None, Some("secret"), true)
        .unwrap();
    f.connect_a();
    f.service().disconnect("b").unwrap();
    assert_eq!(f.cfg.active_id(), Some("a"));
    assert!(!members(&f.cfg, &f.registry, &f.store)
        .unwrap()
        .contains("b"));
    assert!(!f.store.status("b").unwrap().present);
}

#[test]
fn supporter_connections_disconnect_active_stops_supporter_no_silent_fallback() {
    let mut f = Fixture::new(TokenMode::Optional);
    f.connect_a();
    f.service()
        .connect("b", None, Some("secret"), true)
        .unwrap();
    f.service().disconnect("b").unwrap();
    assert_eq!(f.cfg.adapter, None);
    assert!(f.cfg.connections.contains_key("a"));
    assert!(crate::configured_model(&f.cfg, &f.registry).is_none());
    assert!(!crate::supporter_connected(
        &f.cfg,
        &f.registry,
        &super::super::TokenSnapshot {
            usable: true,
            acl_invalid: false,
        }
    ));
}

#[test]
fn supporter_connections_disconnect_keeps_usage_history() {
    let mut f = Fixture::new(TokenMode::None);
    f.connect_a();
    std::fs::write(f.dir.path().join("spend.json"), "{}").unwrap();
    std::fs::write(f.dir.path().join("journal.jsonl"), "opaque-history").unwrap();
    f.service().disconnect("a").unwrap();
    assert_eq!(
        std::fs::read_to_string(f.dir.path().join("spend.json")).unwrap(),
        "{}"
    );
    assert_eq!(
        std::fs::read_to_string(f.dir.path().join("journal.jsonl")).unwrap(),
        "opaque-history"
    );
}

#[test]
fn supporter_connections_list_never_contains_token_value() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", None, Some("secret-only-write"), true)
        .unwrap();
    let rows = list(&f.cfg, &f.registry, &f.store).unwrap();
    assert_eq!(rows[0]["token"]["present"], true);
    assert!(!serde_json::to_string(&rows)
        .unwrap()
        .contains("secret-only-write"));
}

#[test]
fn supporter_connections_legacy_active_adapter_with_stored_token_is_connected_and_active() {
    let mut f = Fixture::new(TokenMode::Required);
    f.registry.register("typesafe-jev", f.b.clone());
    f.cfg = toml::from_str("adapter = 'typesafe-jev'").unwrap();
    f.store.set("typesafe-jev", "private-secret").unwrap();
    let rows = list(&f.cfg, &f.registry, &f.store).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["active"], true);
    assert_eq!(rows[0]["model"], "one");
    assert!(crate::supporter_connected(
        &f.cfg,
        &f.registry,
        &super::super::TokenSnapshot::read(&f.store, "typesafe-jev")
    ));
    assert!(crate::configured_model(&f.cfg, &f.registry).is_some());
}

#[test]
fn supporter_connections_stored_token_without_table_entry_is_a_connection() {
    let f = Fixture::new(TokenMode::Optional);
    f.store.set("b", "private-secret").unwrap();
    let rows = list(&f.cfg, &f.registry, &f.store).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["adapter"], "b");
    assert_eq!(rows[0]["active"], false);
}

#[test]
fn supporter_connections_absent_adapter_means_no_active() {
    let f = Fixture::new(TokenMode::None);
    assert_eq!(f.cfg.active_id(), None);
    assert!(list(&f.cfg, &f.registry, &f.store).unwrap().is_empty());
}

#[test]
fn supporter_connections_failed_config_save_restores_live_model_and_disconnect_token() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", None, Some("private-secret"), true)
        .unwrap();
    let previous = f.cfg.clone();
    let mut service = Connections {
        cfg: &mut f.cfg,
        registry: &f.registry,
        store: &f.store,
        persist: |_: &SupporterConfig| Err(ConnectionError::unavailable()),
    };
    assert!(service.set_model("b", "two".into()).is_err());
    assert!(service.disconnect("b").is_err());
    assert_eq!(f.b.model_id().as_deref(), Some("one"));
    assert_eq!(f.cfg, previous);
    assert!(f.store.status("b").unwrap().present);
}

#[test]
fn supporter_connections_invalid_model_and_unknown_adapter_have_no_side_effects() {
    let mut f = Fixture::new(TokenMode::Required);
    f.connect_a();
    let previous = f.cfg.clone();
    assert_eq!(
        f.service()
            .connect("b", Some("missing".into()), Some("secret"), true)
            .unwrap_err()
            .code,
        "invalid_model"
    );
    assert_eq!(
        f.service().activate("missing").unwrap_err().code,
        "unknown_adapter"
    );
    assert_eq!(f.cfg, previous);
    assert!(!f.store.status("b").unwrap().present);
}

#[test]
fn supporter_connections_disconnect_corrupt_token_still_removes_connection() {
    let mut f = Fixture::new(TokenMode::Required);
    f.service()
        .connect("b", None, Some("secret"), true)
        .unwrap();
    std::fs::write(f.store.path("b"), "invalid-json").unwrap();
    f.service().disconnect("b").unwrap();
    assert_eq!(f.cfg.active_id(), None);
    assert!(!f.store.status("b").unwrap().present);
}

#[test]
fn supporter_connections_use_connects_or_activates_with_per_adapter_model() {
    let mut f = Fixture::new(TokenMode::Required);
    f.store.set("b", "secret").unwrap();
    f.service().use_adapter("b", Some("two".into())).unwrap();
    f.connect_a();
    f.service().use_adapter("b", None).unwrap();
    assert_eq!(f.cfg.connection("b").model.as_deref(), Some("two"));
    assert_eq!(f.cfg.connection("a").model.as_deref(), Some("one"));
}

#[test]
fn supporter_connections_unregistered_corrupt_token_is_still_saved() {
    let f = Fixture::new(TokenMode::Optional);
    std::fs::write(f.store.path("disabled"), "invalid-json").unwrap();
    let rows = list(&f.cfg, &f.registry, &f.store).unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["adapter"], "disabled");
    assert_eq!(rows[0]["token"]["present"], true);
    assert_eq!(rows[0]["token_mode"], Value::Null);
    assert_eq!(rows[0]["error"]["code"], "unavailable");
}

#[test]
fn supporter_connections_storage_namespace_cannot_delete_usage_ledger() {
    let mut f = Fixture::new(TokenMode::None);
    std::fs::write(f.dir.path().join("spend.json"), "{}").unwrap();
    assert!(f.store.set("spend", "secret").is_err());
    assert!(f.store.clear("spend").is_err());
    assert!(f.service().disconnect("spend").is_err());
    assert_eq!(
        std::fs::read_to_string(f.dir.path().join("spend.json")).unwrap(),
        "{}"
    );
}

#[test]
fn supporter_connections_clear_token_keeps_saved_membership_model_and_active() {
    let mut f = Fixture::new(TokenMode::Required);
    f.store.set("b", "secret").unwrap();
    f.service().clear_token("b").unwrap();
    assert!(!f.store.status("b").unwrap().present);
    assert!(members(&f.cfg, &f.registry, &f.store)
        .unwrap()
        .contains("b"));
    f.store.set("b", "secret").unwrap();
    f.service().use_adapter("b", Some("two".into())).unwrap();
    f.service().clear_token("b").unwrap();
    assert_eq!(f.cfg.active_id(), Some("b"));
    assert_eq!(f.cfg.connection("b").model.as_deref(), Some("two"));
}

#[test]
fn supporter_connections_legacy_first_catalogue_model_is_verified_before_running() {
    let mut f = Fixture::new(TokenMode::None);
    f.cfg.adapter = Some("a".into());
    *f.a.selected.lock() = "two".into();
    f.a.unavailable.store(true, Ordering::SeqCst);
    assert!(!crate::restore_model(
        &f.cfg,
        "a",
        &f.registry.get("a").unwrap()
    ));
    assert!(crate::configured_model(&f.cfg, &f.registry).is_none());
    assert_eq!(
        list(&f.cfg, &f.registry, &f.store).unwrap()[0]["error"]["code"],
        "unavailable"
    );
    f.a.unavailable.store(false, Ordering::SeqCst);
    assert!(crate::restore_model(
        &f.cfg,
        "a",
        &f.registry.get("a").unwrap()
    ));
    assert_eq!(
        crate::configured_model(&f.cfg, &f.registry)
            .unwrap()
            .model_id()
            .as_deref(),
        Some("one")
    );
}
