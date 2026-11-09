use std::sync::{mpsc, Arc};
use std::time::{Duration, Instant};

use codeterm_decision::{DecisionModel, TokenMode};
use serde_json::json;

use super::JsProvider;
use crate::js_runtime::vm::{call_json, PluginVm, VmError};
use crate::js_runtime::{call_gate, JsDecisionModel, PluginManifest};
use crate::model_catalog;
use crate::provider::AgentProvider;

const PLUGIN: &str = "discovery-deadline-fixture";
// A failure boundary for a wedged expectation, never evidence of success.
const WEDGED: Duration = Duration::from_secs(3);

fn source(discovery: &str, ordinary: &str) -> String {
    format!(
        r#"
module.exports.default = {{
  discoverModels() {{
    return host.io({{op:"testGate",entered:"{discovery}.entered",release:"{discovery}.release",completed:"{discovery}.completed"}},
      function () {{ return [{{ id: "fixture/live", displayName: "Live" }}]; }});
  }},
  ordinary() {{
    return host.io({{op:"testGate",entered:"{ordinary}.entered",release:"{ordinary}.release",completed:"{ordinary}.completed"}},
      function () {{ return true; }});
  }},
  ping() {{ return true; }},
  models() {{ return [{{ id: "fixture/cached", display_name: "Cached" }}]; }},
}};
"#
    )
}

struct ReleaseOnDrop(Vec<String>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        for gate in &self.0 {
            call_gate::open(gate);
        }
    }
}

fn manifest() -> PluginManifest {
    serde_json::from_value(json!({
        "id": PLUGIN,
        "version": "0.0.0",
        "displayName": "Discovery deadline",
        "hostIoJobs": true
    }))
    .expect("manifest")
}

#[test]
fn slow_discovery_outlives_the_standard_bound_without_blocking_the_plugin() {
    let discovery = "discovery-deadline.discovery";
    let ordinary = "discovery-deadline.ordinary";
    for name in ["entered", "release", "completed"] {
        call_gate::reset(&format!("{discovery}.{name}"));
        call_gate::reset(&format!("{ordinary}.{name}"));
    }
    // The scaled-down standard bound stands in for the 5 s production bound.
    crate::js_runtime::host_exec::set_test_wait_bound(PLUGIN, Duration::from_millis(100));
    model_catalog::cleanup(PLUGIN);
    model_catalog::put_live(
        PLUGIN,
        vec![model_catalog::model("fixture/cached", "Cached")],
        None,
    );
    let manifest = manifest();
    let vm = Arc::new(
        PluginVm::from_source_with(
            PLUGIN,
            source(discovery, ordinary),
            Some(serde_json::to_string(&manifest).unwrap()),
        )
        .expect("vm"),
    );
    let decision = Arc::new(JsDecisionModel::new(vm.clone(), PLUGIN, TokenMode::None));
    let cached = decision.models();
    assert_eq!(cached.len(), 1);
    let provider: Arc<dyn AgentProvider> = Arc::new(JsProvider::new(manifest, vm.clone()));
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    // Declared after the runtime so a failed assertion releases the held jobs before the
    // runtime's drop waits for its blocking discovery.
    let _release = ReleaseOnDrop(vec![
        format!("{discovery}.release"),
        format!("{ordinary}.release"),
    ]);
    let refresh = runtime.spawn(model_catalog::refresh_live(
        provider,
        model_catalog::DISCOVER_TIMEOUT,
    ));
    assert!(
        call_gate::wait_opened(&format!("{discovery}.entered"), WEDGED),
        "discovery never reached its host I/O"
    );

    let started = Instant::now();
    let timed_out = call_json::<bool>(&vm, "ordinary", &[]);
    let elapsed = started.elapsed();
    call_gate::open(&format!("{ordinary}.release"));
    // Closing a job at its deadline runs the job's cancel hook, which revokes the call.
    assert!(
        matches!(timed_out, Err(VmError::CallCancelled)) && elapsed < WEDGED,
        "an ordinary call must end at the standard bound: {timed_out:?} after {elapsed:?}"
    );
    assert!(
        call_json::<bool>(&vm, "ping", &[]).expect("a call between discovery frames"),
        "the plugin VM must stay free while discovery waits on host I/O"
    );
    assert_eq!(
        model_catalog::resolve_live_models(PLUGIN)
            .map(|models| models.into_iter().map(|model| model.id).collect::<Vec<_>>()),
        Some(vec!["fixture/cached".to_string()])
    );
    let (tx, rx) = mpsc::channel();
    let reader = decision.clone();
    std::thread::spawn(move || tx.send(reader.models()).unwrap());
    let read = rx.recv_timeout(WEDGED);
    let still_running = !refresh.is_finished();
    call_gate::open(&format!("{discovery}.release"));
    assert_eq!(
        read.expect("catalogue reader queued behind discovery"),
        cached
    );
    assert!(
        still_running,
        "discovery ended before its host I/O was released"
    );

    let published = runtime
        .block_on(refresh)
        .unwrap()
        .expect("the slow discovery must publish");
    assert_eq!(published[0].id, "fixture/live");
    assert_eq!(
        model_catalog::resolve_live_models(PLUGIN).map(|models| models[0].id.clone()),
        Some("fixture/live".to_string())
    );
    model_catalog::cleanup(PLUGIN);
}
