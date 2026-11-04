use codeterm_service_core::agent::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, TaskDelivery,
};
use std::path::PathBuf;
use std::time::Duration;

#[tokio::test]
async fn oversized_theme_is_rejected_before_parsing_or_writing() {
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(
        codeterm_service_core::multiplexer::state::MultiplexerState::empty(),
    );
    let state = crate::test_support::app_state(&fixture);
    let result = super::save_theme_api(
        axum::extract::State(state),
        Default::default(),
        axum::Json(super::SaveThemeBody {
            id: "bounded-theme".into(),
            toml: "!".repeat(super::MAX_THEME_BYTES + 1),
        }),
    )
    .await;
    assert_eq!(
        result.unwrap_err().0,
        axum::http::StatusCode::PAYLOAD_TOO_LARGE
    );
}

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
