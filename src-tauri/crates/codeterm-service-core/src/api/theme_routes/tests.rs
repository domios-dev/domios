use crate::agent::provider::{AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery};
use crate::agent::registry::AgentRegistry;
use std::path::PathBuf;
use std::time::Duration;

pub(crate) struct SettingsProbe {
    pub(crate) path: PathBuf,
}

impl AgentProvider for SettingsProbe {
    fn id(&self) -> &str {
        "fixture"
    }
    fn display_name(&self) -> &str {
        "Fixture"
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
    fn check_rule_status(&self) -> serde_json::Value {
        serde_json::json!({"installed": self.path.exists(), "update_available": false})
    }
    fn install_rule(&self) -> Result<(), String> {
        std::fs::write(&self.path, b"rule").map_err(|error| error.to_string())
    }
    fn restore_integration(&self) -> Result<(), String> {
        std::fs::copy(self.path.with_extension("backup"), &self.path)
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

#[test]
fn provider_settings_contract_restore_dispatch_persists_provider_result() {
    let temporary = tempfile::tempdir().unwrap();
    let path = temporary.path().join("integration");
    std::fs::write(path.with_extension("backup"), b"original").unwrap();
    std::fs::write(&path, b"modified").unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(SettingsProbe { path: path.clone() }));
    crate::service::provider_hooks::restore_provider_integration_core(&registry, "fixture")
        .unwrap();
    assert_eq!(std::fs::read(path).unwrap(), b"original");
    assert_eq!(
        crate::service::provider_hooks::restore_provider_integration_core(&registry, "unknown"),
        Err("Unknown provider: unknown".into())
    );
}
