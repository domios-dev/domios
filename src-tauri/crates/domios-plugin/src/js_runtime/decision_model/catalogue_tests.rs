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
        let vm = Arc::new(
            PluginVm::from_source_with(&format!("catalogue-{}", manifest.id), source, Some(raw))
                .unwrap(),
        );
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
