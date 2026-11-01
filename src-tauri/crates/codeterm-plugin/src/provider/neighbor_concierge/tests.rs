use super::*;
use crate::provider::{
    AgentLaunchParams, ExecutionSandboxCapability, ExecutionSandboxMode, ReadinessStrategy,
};

struct Fixture {
    delivery: SystemPromptDelivery,
    sandbox: Option<ExecutionSandboxCapability>,
}
#[async_trait::async_trait]
impl AgentProvider for Fixture {
    fn id(&self) -> &str {
        "opaque-provider"
    }
    fn display_name(&self) -> &str {
        "Fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::InlineFlag
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        self.delivery.clone()
    }
    fn execution_sandbox_capability(&self) -> Option<ExecutionSandboxCapability> {
        self.sandbox.clone()
    }
}

#[test]
fn platform_projection_distinguishes_enforced_and_advisory_and_excludes_no_tools() {
    let mut provider = Fixture {
        delivery: SystemPromptDelivery::SessionConfig,
        sandbox: Some(ExecutionSandboxCapability {
            platforms: vec!["fixture-os".into()],
            read_only: Some(ExecutionSandboxMode {
                launch_args: vec!["opaque-read".into()],
                runtime_mode: None,
            }),
            mutating: Some(ExecutionSandboxMode {
                launch_args: vec!["opaque-folder".into()],
                runtime_mode: None,
            }),
            ..Default::default()
        }),
    };
    let enforced = describe(&provider, "fixture-os");
    assert!(enforced.supported);
    assert_eq!(enforced.machine_access, ["read_only", "folder", "full"]);
    assert_eq!(enforced.enforced_machine_access, ["read_only", "folder"]);
    let advisory = describe(&provider, "other-os");
    assert_eq!(advisory.machine_access, enforced.machine_access);
    assert!(advisory.enforced_machine_access.is_empty());
    provider.delivery = SystemPromptDelivery::External;
    assert_eq!(
        describe(&provider, "fixture-os"),
        NeighborConciergeCapability::default()
    );
}
