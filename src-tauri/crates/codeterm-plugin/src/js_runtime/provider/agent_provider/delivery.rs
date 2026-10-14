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
