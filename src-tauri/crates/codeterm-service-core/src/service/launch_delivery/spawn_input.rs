use super::{
    mode_for_descriptor, FallbackDeliverySpec, LaunchDeliveryMode, VisibleSpawnFirstTurn,
    VisibleSpawnTaskSelection,
};
use crate::agent::provider::{AgentProvider, StarterPrompt, TaskDelivery};

pub(crate) fn select_visible_spawn_task<'a>(
    role: Option<&str>,
    task: Option<&'a str>,
    system_prompt_capable: bool,
    native_starter_capable: bool,
) -> VisibleSpawnTaskSelection<'a> {
    let task = task.filter(|text| !text.trim().is_empty());
    match (role, task, system_prompt_capable, native_starter_capable) {
        (None, Some(task), true, true) => VisibleSpawnTaskSelection {
            provider_launch: Some(task),
            provider_starter: Some(crate::agent::provider::StarterPrompt::AssignedTask),
            confirmed_first_turn: None,
        },
        (None, Some(task), true, false) => VisibleSpawnTaskSelection {
            provider_launch: Some(task),
            provider_starter: None,
            confirmed_first_turn: Some(VisibleSpawnFirstTurn::AssignedTaskStarter),
        },
        (None, Some(task), false, _) => VisibleSpawnTaskSelection {
            provider_launch: None,
            provider_starter: None,
            confirmed_first_turn: Some(VisibleSpawnFirstTurn::Task(task)),
        },
        _ => VisibleSpawnTaskSelection {
            provider_launch: task,
            provider_starter: None,
            confirmed_first_turn: None,
        },
    }
}

fn visible_spawn_capabilities(
    provider: &dyn crate::agent::provider::AgentProvider,
) -> (bool, bool) {
    (
        matches!(
            mode_for_descriptor(&provider.system_prompt_delivery()),
            LaunchDeliveryMode::SystemPrompt
        ),
        provider
            .starter_prompt_text(crate::agent::provider::StarterPrompt::AssignedTask)
            .is_some(),
    )
}

#[derive(Debug, PartialEq, Eq)]
pub struct SpawnInput<'a> {
    pub provider_task: Option<&'a str>,
    pub starter_prompt: StarterPrompt,
    pub confirmed_first_turn: Option<String>,
    pub task_needs_fallback: bool,
}

pub fn prepare_spawn_input<'a>(
    provider: Option<&dyn AgentProvider>,
    role: Option<&str>,
    task: Option<&'a str>,
    initial_starter: StarterPrompt,
    startup_handshake: Option<StarterPrompt>,
    frame_task: impl FnOnce(&str) -> String,
) -> SpawnInput<'a> {
    let (system_capable, native_starter) = provider
        .map(visible_spawn_capabilities)
        .unwrap_or((false, false));
    let selection = select_visible_spawn_task(role, task, system_capable, native_starter);
    let starter_prompt = crate::service::startup_turn::visible_spawn_starter_prompt(
        startup_handshake,
        selection.provider_starter,
    )
    .unwrap_or(initial_starter);
    let confirmed_first_turn = if let Some(prompt) = startup_handshake {
        super::startup_handshake_confirmed_first_turn(native_starter).map(|_| {
            super::mark_starter_prompt(
                prompt,
                provider
                    .and_then(|p| p.starter_prompt_text(prompt))
                    .unwrap_or_default(),
            )
        })
    } else {
        selection.confirmed_first_turn.map(|turn| match turn {
            VisibleSpawnFirstTurn::AssignedTaskStarter => super::mark_starter_prompt(
                StarterPrompt::AssignedTask,
                super::assigned_task_starter(
                    provider
                        .and_then(|p| p.starter_prompt_text(StarterPrompt::AssignedTask))
                        .as_deref(),
                ),
            ),
            VisibleSpawnFirstTurn::Task(task) => frame_task(task),
        })
    };
    let task_needs_fallback = provider
        .map(|p| matches!(p.task_delivery(), TaskDelivery::PtyInput) && !system_capable)
        .unwrap_or(true);
    log::debug!("[spawn-input] role={role:?} task_present={} system_capable={system_capable} native_starter={native_starter} starter={starter_prompt:?} confirmed_first_turn={} task_needs_fallback={task_needs_fallback}", task.is_some_and(|t| !t.trim().is_empty()), confirmed_first_turn.is_some());
    SpawnInput {
        provider_task: selection.provider_launch,
        starter_prompt,
        confirmed_first_turn,
        task_needs_fallback,
    }
}

impl SpawnInput<'_> {
    pub(crate) fn fallback_spec(
        &self,
        bootstrap: Option<&str>,
        launch_task: Option<&str>,
        starter_text: Option<&str>,
    ) -> Option<FallbackDeliverySpec> {
        super::fallback_delivery_spec(
            bootstrap.filter(|_| self.task_needs_fallback),
            self.confirmed_first_turn
                .as_deref()
                .or_else(|| self.task_needs_fallback.then_some(launch_task).flatten()),
            starter_text,
        )
    }
}
