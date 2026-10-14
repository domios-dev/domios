use super::super::SpawnAgentBody;
use codeterm_service_core::agent::provider::{
    AgentLaunchParams, AgentProvider, ReadinessStrategy, StarterPrompt, SystemPromptDelivery,
    TaskDelivery,
};
use codeterm_service_core::service::launch_delivery_prepare_spawn_input;
use std::{path::PathBuf, time::Duration};

struct InputProvider {
    system: bool,
    starter: Option<&'static str>,
}

impl AgentProvider for InputProvider {
    fn id(&self) -> &str {
        "input-fixture"
    }
    fn display_name(&self) -> &str {
        "Input Fixture"
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
    fn system_prompt_delivery(&self) -> SystemPromptDelivery {
        if self.system {
            SystemPromptDelivery::CliFlag("--fixture-system".into())
        } else {
            SystemPromptDelivery::External
        }
    }
    fn starter_prompt_text(&self, prompt: StarterPrompt) -> Option<String> {
        (prompt == StarterPrompt::AssignedTask)
            .then_some(self.starter)
            .flatten()
            .map(str::to_string)
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
    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
}

fn assert_shared_input(provider: InputProvider, task: &str) {
    let request = serde_json::json!({"agent": provider.id(), "cwd": "/fixture", "task": task});
    let local = codeterm_service_core::service::SpawnAgentParams::from_json(&request).unwrap();
    let mesh: SpawnAgentBody = serde_json::from_value(request).unwrap();
    let local_input = prepare_spawn_input(
        Some(&provider),
        local.role.as_deref(),
        local.task.as_deref(),
        local.starter_prompt,
        None,
        str::to_string,
    );
    let mesh_input = prepare_spawn_input(
        Some(&provider),
        mesh.role.as_deref(),
        mesh.task.as_deref(),
        codeterm_service_core::service::starter_prompt_for_role(mesh.role.as_deref()),
        None,
        str::to_string,
    );
    assert_eq!(local_input, mesh_input);
    let fallback = mesh_input.fallback_spec(
        None,
        mesh_input.provider_task,
        provider
            .starter_prompt_text(mesh_input.starter_prompt)
            .as_deref(),
    );
    let native = provider.starter_prompt_text(mesh_input.starter_prompt);
    assert_eq!(
        usize::from(fallback.is_some()) + usize::from(native.is_some()),
        1
    );
    if provider.system {
        assert_eq!(mesh_input.provider_task, Some(task));
        if let Some(starter) = provider.starter {
            assert_eq!(mesh_input.starter_prompt, StarterPrompt::AssignedTask);
            assert_eq!(native.as_deref(), Some(starter));
            assert!(fallback.is_none());
        } else {
            assert_eq!(mesh_input.starter_prompt, StarterPrompt::NoStarter);
            assert_eq!(
                fallback.unwrap().payload,
                mesh_input.confirmed_first_turn.unwrap()
            );
        }
    } else {
        assert!(mesh_input.provider_task.is_none());
        assert_eq!(fallback.unwrap().payload, task);
        assert_eq!(mesh_input.confirmed_first_turn.as_deref(), Some(task));
    }
}

#[test]
fn mesh_and_local_spawn_share_first_turn_input() {
    assert_shared_input(
        InputProvider {
            system: false,
            starter: None,
        },
        "opaque-task-\n$(literal)-`token`-λ",
    );
}

#[test]
fn mesh_and_local_spawn_share_assigned_task_starter() {
    for starter in [Some("opaque-starter-token"), None] {
        assert_shared_input(
            InputProvider {
                system: true,
                starter,
            },
            "opaque-system-task-token",
        );
    }
}
