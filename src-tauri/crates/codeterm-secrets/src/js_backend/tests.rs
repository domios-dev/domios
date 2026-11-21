use super::*;
use std::sync::{mpsc, Mutex};
use std::time::Duration;

/// Failure bound for a wedged expectation; every wait is released by a signal.
const WEDGED: Duration = Duration::from_secs(30);

struct ParkedVault {
    entered: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
    release: Mutex<mpsc::Receiver<()>>,
}

impl PluginCalls for ParkedVault {
    fn has_method(&self, _: &str) -> bool {
        true
    }

    fn call(&self, method: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        assert_eq!(method, "secretGetItem");
        if let Some(entered) = self.entered.lock().unwrap().take() {
            let _ = entered.send(());
        }
        self.release
            .lock()
            .unwrap()
            .recv_timeout(WEDGED)
            .expect("the parked lookup was never released");
        Ok(json!({ "ok": { "item_id": "id", "name": "token", "value": "v", "notes": null } }))
    }
}

#[test]
fn a_parked_vault_lookup_leaves_the_async_runtime_serving_other_tasks() {
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (progress_tx, progress_rx) = mpsc::channel();
    let backend = JsSecretBackend::new(
        Arc::new(ParkedVault {
            entered: Mutex::new(Some(entered_tx)),
            release: Mutex::new(release_rx),
        }),
        "parked-vault",
        false,
    );

    let runtime = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .build()
            .expect("runtime");
        runtime.block_on(async move {
            let lookup = tokio::spawn(async move { backend.get("token").await });
            entered_rx.await.expect("the lookup reached the vault");
            progress_tx.send(()).expect("test thread listening");
            lookup.await.expect("lookup task")
        })
    });

    progress_rx
        .recv_timeout(WEDGED)
        .expect("the runtime stalled behind a parked vault lookup");
    release_tx.send(()).expect("lookup parked");
    let secret = runtime
        .join()
        .expect("runtime thread")
        .expect("lookup result");
    assert_eq!(secret.value.expose_secret(), "v");
}

const SENTINEL: &str = "SENTINEL-VALUE-7f3a";

#[test]
fn active_backend_reload_preserves_an_in_flight_lookup() {
    let (entered_tx, entered_rx) = tokio::sync::oneshot::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let registry = Arc::new(crate::SecretRegistry::new());
    registry.register(
        "parked-vault",
        Arc::new(JsSecretBackend::new(
            Arc::new(ParkedVault {
                entered: Mutex::new(Some(entered_tx)),
                release: Mutex::new(release_rx),
            }),
            "parked-vault",
            false,
        )),
    );
    let active = crate::build_active_store(
        &codeterm_config::schema::SecretsConfig {
            backend: "parked-vault".into(),
            ..Default::default()
        },
        &registry,
    );
    runtime().block_on(async {
        let first = active.clone();
        let lookup = tokio::spawn(async move { first.get("token").await });
        entered_rx.await.expect("lookup reached the old backend");
        registry.register(
            "parked-vault",
            Arc::new(backend(Scripted(json!({
                "ok": {"item_id": "new", "name": "token", "value": "new"}
            })))),
        );
        let next = active.get("token").await;
        release_tx.send(()).expect("old lookup parked");
        assert_eq!(next.unwrap().value.expose_secret(), "new");
        assert_eq!(lookup.await.unwrap().unwrap().value.expose_secret(), "v");
    });
}

struct Scripted(serde_json::Value);

impl PluginCalls for Scripted {
    fn has_method(&self, _: &str) -> bool {
        true
    }

    fn call(&self, _: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        Ok(self.0.clone())
    }
}

struct Throwing;

impl PluginCalls for Throwing {
    fn has_method(&self, _: &str) -> bool {
        true
    }

    fn call(&self, _: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        Err(VmError::Script(format!(
            "bw exited 1: {{\"password\":\"{SENTINEL}\"}}"
        )))
    }
}

fn backend(plugin: impl PluginCalls) -> JsSecretBackend {
    JsSecretBackend::new(Arc::new(plugin), "scripted-vault", false)
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("runtime")
}

fn leaked_items() -> serde_json::Value {
    json!([{ "item_id": "i1", "name": "token", "login": { "password": SENTINEL }, "notes": SENTINEL }])
}

#[test]
fn a_backend_error_carrying_item_values_never_reaches_the_error() {
    let rt = runtime();
    let envelope = json!({ "error": { "kind": "backend", "message": leaked_items().to_string() } });
    let errors = [
        rt.block_on(backend(Scripted(envelope)).list(ListFilter::default()))
            .unwrap_err(),
        rt.block_on(backend(Throwing).list(ListFilter::default()))
            .unwrap_err(),
        rt.block_on(
            backend(Scripted(json!({ "ok": { "value": SENTINEL } }))).list(ListFilter::default()),
        )
        .unwrap_err(),
        rt.block_on(backend(Scripted(leaked_items())).list(ListFilter::default()))
            .unwrap_err(),
        rt.block_on(backend(Throwing).get("token")).unwrap_err(),
        rt.block_on(backend(Throwing).delete("token")).unwrap_err(),
        rt.block_on(backend(Throwing).unlock(UnlockCredentials::with_password("pw".into())))
            .unwrap_err(),
    ];
    for err in errors {
        assert!(matches!(err, SecretError::Backend { .. }));
        assert!(!format!("{err} {err:?}").contains(SENTINEL));
    }
    let status = rt.block_on(backend(Throwing).status());
    assert!(!format!("{status:?}").contains(SENTINEL));
}

#[test]
fn a_successful_list_with_value_fields_returns_metadata_only() {
    let items = runtime()
        .block_on(backend(Scripted(json!({ "ok": leaked_items() }))).list(ListFilter::default()))
        .expect("list");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].item_id, "i1");
    assert!(!format!("{items:?}").contains(SENTINEL));
}

fn nested_bw_list() -> serde_json::Value {
    crate::redact::tests::nested_bw_list()
}

#[test]
fn a_nested_vault_list_decodes_to_metadata_for_list_and_search() {
    let rt = runtime();
    let wrapped = json!({ "ok": nested_bw_list() });
    let listed = rt
        .block_on(backend(Scripted(wrapped.clone())).list(ListFilter::default()))
        .expect("list");
    let found = rt
        .block_on(backend(Scripted(wrapped)).search("token"))
        .expect("search");
    for items in [listed, found] {
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].item_id, "i1");
        assert!(!format!("{items:?}").contains(SENTINEL));
    }
}

#[test]
fn an_unwrapped_vault_list_is_refused_as_a_typed_error_without_its_payload() {
    let rt = runtime();
    let errors = [
        rt.block_on(backend(Scripted(nested_bw_list())).list(ListFilter::default()))
            .unwrap_err(),
        rt.block_on(backend(Scripted(nested_bw_list())).search("token"))
            .unwrap_err(),
    ];
    for err in errors {
        assert_eq!(err.kind(), "backend");
        assert!(!format!("{err} {err:?}").contains(SENTINEL));
    }
}

struct SequencedPlugin {
    calls: Mutex<Vec<String>>,
    responses: Mutex<std::collections::VecDeque<Result<serde_json::Value, VmError>>>,
    supports_sync: bool,
}

impl SequencedPlugin {
    fn new(responses: Vec<Result<serde_json::Value, VmError>>, supports_sync: bool) -> Arc<Self> {
        Arc::new(Self {
            calls: Mutex::new(Vec::new()),
            responses: Mutex::new(responses.into()),
            supports_sync,
        })
    }

    fn recorded_calls(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }
}

impl PluginCalls for SequencedPlugin {
    fn has_method(&self, method: &str) -> bool {
        method != "secretSync" || self.supports_sync
    }

    fn call(&self, method: &str, _: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        self.calls.lock().unwrap().push(method.to_string());
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| panic!("unexpected plugin call: {method}"))
    }
}

fn save_request(name: &str, value: &str) -> SaveRequest {
    SaveRequest {
        name: name.into(),
        value: SecretString::from(value),
        notes: Some("rotation note".into()),
        folder: Some("media".into()),
        organization: None,
        collection: None,
        overwrite: true,
    }
}

#[test]
fn uncertain_save_verifies_via_readback_without_replaying_mutation() {
    let plugin = SequencedPlugin::new(
        vec![
            Err(VmError::IoUnconfirmed),
            Ok(json!({
                "ok": {
                    "item_id": "bw-123",
                    "name": "media-api-key",
                    "value": "expected-value",
                    "notes": "rotation note"
                }
            })),
        ],
        true,
    );
    let store = JsSecretBackend::new(plugin.clone(), "scripted-vault", false);
    let saved = runtime()
        .block_on(store.save(save_request("media-api-key", "expected-value")))
        .expect("readback confirms remote write");
    assert_eq!(saved.item_id, "bw-123");
    assert_eq!(saved.name, "media-api-key");
    assert_eq!(saved.folder.as_deref(), Some("media"));
    assert_eq!(
        plugin.recorded_calls(),
        vec!["secretSave".to_string(), "secretGetItem".to_string()]
    );
}

#[test]
fn uncertain_save_syncs_once_when_local_cache_is_stale() {
    let plugin = SequencedPlugin::new(
        vec![
            Ok(json!({ "error": { "kind": "backend", "message": "bw failed" } })),
            Ok(json!({ "error": { "kind": "not_found" } })),
            Ok(json!({ "ok": true })),
            Ok(json!({
                "ok": {
                    "item_id": "bw-456",
                    "name": "media-api-key",
                    "value": "expected-value",
                    "notes": "rotation note"
                }
            })),
        ],
        true,
    );
    let store = JsSecretBackend::new(plugin.clone(), "scripted-vault", false);
    let saved = runtime()
        .block_on(store.save(save_request("media-api-key", "expected-value")))
        .expect("sync + readback confirms remote write");
    assert_eq!(saved.item_id, "bw-456");
    assert_eq!(
        plugin.recorded_calls(),
        vec![
            "secretSave".to_string(),
            "secretGetItem".to_string(),
            "secretSync".to_string(),
            "secretGetItem".to_string(),
        ]
    );
}

#[test]
fn unconfirmed_save_returns_typed_write_uncertain_without_leaking_payload() {
    let plugin = SequencedPlugin::new(
        vec![
            Ok(json!({
                "error": { "kind": "backend", "message": leaked_items().to_string() }
            })),
            Ok(json!({
                "ok": {
                    "item_id": "bw-789",
                    "name": "media-api-key",
                    "value": "stale-old-value",
                    "notes": "rotation note"
                }
            })),
            Ok(json!({ "ok": true })),
            Ok(json!({
                "ok": {
                    "item_id": "bw-789",
                    "name": "media-api-key",
                    "value": "stale-old-value",
                    "notes": "rotation note"
                }
            })),
        ],
        true,
    );
    let store = JsSecretBackend::new(plugin.clone(), "scripted-vault", false);
    let err = runtime()
        .block_on(store.save(save_request("media-api-key", "expected-value")))
        .unwrap_err();
    assert!(matches!(err, SecretError::WriteUncertain { .. }), "{err:?}");
    assert_eq!(err.kind(), "write_uncertain");
    assert!(!format!("{err} {err:?}").contains(SENTINEL));
    assert!(!format!("{err} {err:?}").contains("expected-value"));
    assert!(err.to_string().contains("domios mem secret get"));
}

#[test]
fn pre_write_save_refusals_return_directly_without_readback() {
    for (envelope, expected_kind) in [
        (
            json!({ "error": { "kind": "already_exists" } }),
            "already_exists",
        ),
        (json!({ "error": { "kind": "locked" } }), "locked"),
        (
            json!({ "error": { "kind": "bad_request", "message": "name required" } }),
            "bad_request",
        ),
    ] {
        let plugin = SequencedPlugin::new(vec![Ok(envelope)], true);
        let store = JsSecretBackend::new(plugin.clone(), "scripted-vault", false);
        let err = runtime()
            .block_on(store.save(save_request("media-api-key", "expected-value")))
            .unwrap_err();
        assert_eq!(err.kind(), expected_kind);
        assert_eq!(plugin.recorded_calls(), vec!["secretSave".to_string()]);
    }
}
