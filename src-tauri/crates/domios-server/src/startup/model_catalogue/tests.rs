use std::path::PathBuf;
use std::sync::{mpsc, Arc};
use std::time::Duration;

use codeterm_plugin::model_catalog::{self, RegisteredDefaults};
use codeterm_plugin::provider::{
    AgentLaunchParams, AgentProvider, ModelInfo, ReadinessStrategy, TaskDelivery,
};
use codeterm_service_core::agent::registry::AgentRegistry;
use parking_lot::Mutex;

// Bounds a broken build only; a fixed startup never touches the probe.
const STARTUP_DEADLINE: Duration = Duration::from_secs(10);

/// A provider whose catalogue-input probe answers only when the test drops
/// the sender, like a CLI resolution or FSEvents stream that hangs.
struct HangingProbe {
    id: String,
    gate: Mutex<mpsc::Receiver<()>>,
}

#[async_trait::async_trait]
impl AgentProvider for HangingProbe {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn model_credential_files(&self) -> Vec<PathBuf> {
        let _ = self.gate.lock().recv();
        Vec::new()
    }
}

fn model(id: &str) -> ModelInfo {
    ModelInfo {
        id: id.into(),
        display_name: id.into(),
        ..Default::default()
    }
}

fn model_ids(provider: &str) -> Vec<String> {
    model_catalog::resolve_models(provider)
        .into_iter()
        .map(|model| model.id)
        .collect()
}

#[test]
fn startup_perf_serves_cached_catalogue_without_waiting_for_watch_probes() {
    let mut env = codeterm_util::test_env::EnvGuard::lock();
    let data = tempfile::tempdir().unwrap();
    env.set("CODETERM_DATA_DIR", data.path());
    let id = "startup-hanging-probe-fixture";
    let seed = RegisteredDefaults {
        models: vec![model("seed")],
        default: Some("seed".into()),
    };
    model_catalog::register(id, seed.clone());
    model_catalog::put_live(id, vec![model("cached")], Some("cached".into()));
    model_catalog::unregister(id);
    model_catalog::register(id, seed);
    assert_eq!(model_ids(id), ["seed"], "restart starts from the seed");

    let (release, gate) = mpsc::channel();
    let registry = Arc::new(AgentRegistry::new());
    registry.register_provider(Box::new(HangingProbe {
        id: id.into(),
        gate: Mutex::new(gate),
    }));
    let (returned_tx, returned_rx) = mpsc::channel();
    let startup_registry = registry.clone();
    std::thread::spawn(move || {
        super::refresh_providers(&startup_registry);
        let _ = returned_tx.send(());
    });
    let returned = returned_rx.recv_timeout(STARTUP_DEADLINE);
    let primed = model_catalog::has_live_snapshot(id);
    let served = model_ids(id);
    drop(release);

    assert!(
        returned.is_ok(),
        "startup waited for a provider's catalogue watch probe"
    );
    // A live snapshot is shown merged over the registered seed.
    assert!(primed, "the cached catalogue is primed at once");
    assert_eq!(served, ["seed", "cached"]);
    model_catalog::unregister(id);
}
