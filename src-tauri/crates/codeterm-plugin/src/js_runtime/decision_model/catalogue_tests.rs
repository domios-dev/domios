use super::*;
use crate::js_runtime::{call_gate, PluginManifest};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn cold_catalogue_shares_one_discovery_without_blocking_cheap_vm_calls() {
    let root = crate::test_support::fixture_manifest_dir().join("../../../plugins");
    let mut exercised = 0;
    for directory in std::fs::read_dir(root).unwrap().flatten() {
        let path = directory.path().join("plugin.json");
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(manifest) = PluginManifest::from_json_str(&raw) else {
            continue;
        };
        if !manifest.model_catalogue.live_discovery_required
            || !manifest.capabilities.decision_model.enabled
        {
            continue;
        }
        exercised += 1;
        let entered = format!("catalogue.{}.entered", manifest.id);
        let release = format!("catalogue.{}.release", manifest.id);
        call_gate::reset(&entered);
        call_gate::reset(&release);
        let source = std::fs::read_to_string(directory.path().join(&manifest.entry)).unwrap();
        let prefix = format!(
            r#"
            let starts = {{}};
            host.exec = function () {{ throw new Error('synchronous discovery'); }};
            host.execStart = function (text) {{
                const key = JSON.stringify(JSON.parse(text).args);
                starts[key] = (starts[key] || 0) + 1;
                const jobId = host.testGateStart({release:?});
                host.testGateOpen({entered:?});
                return JSON.stringify({{ jobId }});
            }};
            const awaitJob = host.awaitJob;
            host.awaitJob = function (job, then) {{
                return awaitJob(job, function (result) {{ result.stdout = 'fixture/model'; return then(result); }});
            }};
            host.fs.fileExists = function () {{ return false; }};
            host.fs.readFile = function () {{ return null; }};
        "#
        );
        let source = prefix
            + &source
            + "; module.exports.default.discoveryCount = function () { return starts; };";
        let vm = Arc::new(PluginVm::from_source_with(&manifest.id, source, Some(raw)).unwrap());
        let model = Arc::new(JsDecisionModel::new(
            vm.clone(),
            manifest.id,
            TokenMode::None,
        ));
        let first_model = model.clone();
        let first = std::thread::spawn(move || first_model.models());
        let entered_ok = call_gate::wait_opened(&entered, Duration::from_secs(3));
        if !entered_ok {
            call_gate::open(&release);
        }
        assert!(entered_ok, "cold discovery did not start");
        assert!(vm.catalogue_gate.try_lock().is_none());
        assert!(model.cached_models().is_empty());
        let second_model = model.clone();
        let second = std::thread::spawn(move || second_model.models());
        let (tx, rx) = mpsc::channel();
        let cheap_vm = vm.clone();
        let cheap = std::thread::spawn(move || {
            tx.send(call_json::<bool>(
                &cheap_vm,
                "detectFromOutput",
                &[json!("opaque")],
            ))
            .unwrap()
        });
        let cheap_result = rx.recv_timeout(Duration::from_secs(1));
        let remained_pending = !call_gate::is_open(&release);
        call_gate::open(&release);
        cheap.join().unwrap();
        assert!(remained_pending);
        assert!(!cheap_result.unwrap().unwrap());
        let models = first.join().unwrap();
        assert_eq!(
            models.first().map(|model| model.id.as_str()),
            Some("fixture/model")
        );
        assert_eq!(models, model.cached_models());
        assert_eq!(models, second.join().unwrap());
        assert_eq!(models, model.models());
        let starts: std::collections::HashMap<String, u32> =
            call_json(&vm, "discoveryCount", &[]).unwrap();
        assert!(!starts.is_empty());
        assert!(starts.values().all(|count| *count == 1));
    }
    assert!(
        exercised > 0,
        "no live-discovery decision provider was exercised"
    );
}

#[test]
fn cached_decision_catalogue_reads_never_resolve_credentials() {
    use crate::host_traits::{ModelProviderEnvUnavailable, PluginHostCtx};
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Host(AtomicUsize);
    impl PluginHostCtx for Host {
        fn model_provider_process_env(
            &self,
            _: &str,
        ) -> Result<HashMap<String, String>, ModelProviderEnvUnavailable> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(HashMap::new())
        }
        fn mem_search_json(&self, _opts_json: &str) -> String {
            unreachable!()
        }
        fn analytics_query_json(&self, _query_json: &str) -> String {
            unreachable!()
        }
        fn workspace_ensure_json(&self, _opts_json: &str) -> String {
            unreachable!()
        }
        fn workspace_tabs_json(&self, _workspace_id: &str) -> String {
            unreachable!()
        }
        fn close_tab(&self, _tab_id: &str) {
            unreachable!()
        }
        fn spawn_tab_json(&self, _spec_json: &str) -> String {
            unreachable!()
        }
        fn worker_get_json(&self, _session_id: &str) -> Option<String> {
            unreachable!()
        }
        fn worker_kill(&self, _session_id: &str) {
            unreachable!()
        }
        fn worker_pick_color(&self) -> String {
            unreachable!()
        }
        fn worker_register_json(&self, _info_json: &str) {
            unreachable!()
        }
        fn worker_start_json(&self, _plugin_id: &str, _opts_json: &str) -> String {
            unreachable!()
        }
        fn worker_poll_json(&self, _session_id: &str) -> String {
            unreachable!()
        }
        fn agent_spawn_json(
            &self,
            _plugin_id: &str,
            _workspace_id: &str,
            _opts_json: &str,
        ) -> String {
            unreachable!()
        }
        fn agent_send_json(&self, _plugin_id: &str, _session_id: &str, _text: &str) -> String {
            unreachable!()
        }
        fn agent_reap_json(&self, _session_id: &str) -> String {
            unreachable!()
        }
        fn agent_poll_json(&self, _session_id: &str) -> String {
            unreachable!()
        }
        fn progress_raw_json(&self, _plugin_id: &str, _opts_json: &str) -> String {
            unreachable!()
        }
        fn contributions_changed(&self, _kind: &str) {
            unreachable!()
        }
        fn emit_graph_worker_spawned_json(&self, _info_json: &str) {
            unreachable!()
        }
        fn socket_display_path(&self) -> String {
            unreachable!()
        }
        fn api_base_url(&self) -> String {
            unreachable!()
        }
        fn api_token(&self) -> Option<String> {
            unreachable!()
        }
        fn secret_get(&self, _plugin_id: &str, _name: &str) -> Option<String> {
            unreachable!()
        }
        fn secret_set(&self, _plugin_id: &str, _name: &str, _value: &str) -> bool {
            unreachable!()
        }
        fn secret_delete(&self, _plugin_id: &str, _name: &str) -> bool {
            unreachable!()
        }
    }
    let host = Arc::new(Host(AtomicUsize::new(0)));
    let context: Arc<dyn PluginHostCtx> = host.clone();
    let vm = Arc::new(
        PluginVm::from_source(
            "cache-fixture",
            r#"module.exports.default = {
        models() { return [{ id: 'opaque-model', display_name: 'Opaque model' }]; },
        modelId() { return 'opaque-model'; },
        selectModel() { return true; },
        decide() { return { type: 'noul', p: 0.8 }; }
    };"#
            .into(),
        )
        .unwrap(),
    );
    assert!(vm.set_service(Arc::downgrade(&context)));
    let model = JsDecisionModel::new(vm, "cache-fixture", TokenMode::None);
    assert!(model.cached_models().is_empty());
    assert_eq!(host.0.load(Ordering::SeqCst), 0);
    let discovered = model.models();
    assert_eq!(host.0.load(Ordering::SeqCst), 1);
    for _ in 0..3 {
        assert_eq!(model.cached_models(), discovered);
    }
    assert_eq!(host.0.load(Ordering::SeqCst), 1);
    assert!(model.select_model("opaque-model"));
    assert_eq!(host.0.load(Ordering::SeqCst), 3);
    model
        .decide(
            &DecisionRequest {
                state: serde_json::Value::Null,
                instructions: "opaque".into(),
                question: codeterm_decision::DecisionQuestion::Noul { criteria: None },
            },
            Instant::now() + Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(host.0.load(Ordering::SeqCst), 4);
}
