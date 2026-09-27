use crate::agent::provider::{StarterPrompt, TaskDelivery};
use crate::service::launch_delivery::FallbackDeliverySpec;

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
) -> Option<String> {
    let turn = fallback_spec
        .map(|spec| spec.payload.clone())
        .or_else(|| {
            registry
                .with_provider(provider_id, |provider| {
                    provider.starter_prompt_text(starter_prompt).or_else(|| {
                        (inline_task_fallback
                            && matches!(provider.task_delivery(), TaskDelivery::InlineFlag))
                        .then(|| task_text.map(str::to_string))
                        .flatten()
                    })
                })
                .flatten()
        })
        .or_else(|| confirmed_first_turn.map(str::to_string));
    let has_hooks = registry
        .with_provider(provider_id, |provider| provider.has_hooks())
        .unwrap_or(false);
    has_hooks.then_some(turn).flatten()
}
