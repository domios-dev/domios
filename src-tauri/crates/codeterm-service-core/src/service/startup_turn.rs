use crate::agent::provider::StarterPrompt;
use crate::service::launch_delivery::FallbackDeliverySpec;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StartupTurnPlan {
    pub expected_turn: Option<String>,
    pub first_turn_will_be_delivered: bool,
}

pub(crate) fn visible_spawn_starter_prompt(
    startup_handshake: Option<StarterPrompt>,
    provider_starter: Option<StarterPrompt>,
) -> Option<StarterPrompt> {
    startup_handshake.or(provider_starter)
}

pub(crate) fn resolve(
    registry: &crate::agent::registry::AgentRegistry,
    provider_id: &str,
    starter_prompt: StarterPrompt,
    fallback_spec: Option<&FallbackDeliverySpec>,
    task_text: Option<&str>,
    inline_task_fallback: bool,
    confirmed_first_turn: Option<&str>,
) -> StartupTurnPlan {
    let provider_plan = registry.with_provider(provider_id, |provider| {
        let launch_task = crate::service::launch_delivery::command_task(provider, task_text);
        let starter_text = provider.starter_prompt_text(starter_prompt);
        let has_command_task = launch_task
            .text
            .as_deref()
            .is_some_and(|task| !task.trim().is_empty());
        let has_starter = starter_text
            .as_deref()
            .is_some_and(|prompt| !prompt.trim().is_empty());
        let first_turn_will_be_delivered = fallback_spec.is_some()
            || has_command_task && (has_starter || launch_task.task_is_first_turn);
        let expected_turn = fallback_spec
            .map(|spec| spec.payload.clone())
            .or_else(|| {
                first_turn_will_be_delivered
                    .then(|| starter_text.clone())
                    .flatten()
            })
            .or_else(|| {
                (first_turn_will_be_delivered
                    && inline_task_fallback
                    && launch_task.task_is_first_turn)
                    .then(|| task_text.map(str::to_string))
                    .flatten()
            })
            .or_else(|| {
                first_turn_will_be_delivered
                    .then(|| confirmed_first_turn.map(str::to_string))
                    .flatten()
            });
        (
            expected_turn,
            first_turn_will_be_delivered,
            provider.has_hooks(),
        )
    });
    let (expected_turn, first_turn_will_be_delivered, has_hooks) =
        provider_plan.unwrap_or((None, fallback_spec.is_some(), false));
    StartupTurnPlan {
        expected_turn: has_hooks.then_some(expected_turn).flatten(),
        first_turn_will_be_delivered,
    }
}
