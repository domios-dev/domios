use super::JsProvider;
use crate::provider::AgentProvider;

pub(super) fn supports_unbound_submit_recovery(provider: &JsProvider) -> bool {
    provider
        .try_call::<bool>("supportsUnboundSubmitRecovery", &[])
        .unwrap_or_else(|| {
            provider
                .native()
                .is_some_and(|native| native.supports_unbound_submit_recovery())
        })
}

pub(super) fn delivery_modal_visible(
    provider: &JsProvider,
    snapshot: &crate::tab_state::TabStateSnapshot,
) -> bool {
    (provider.has_js("parsePrompt") && provider.parse_prompt(&snapshot.screen_text).is_some())
        || crate::provider::provider_classified_delivery_modal_visible(provider, snapshot)
}
