use super::JsProvider;
use crate::provider::InputAttachSignal;

pub(super) fn public_send_waits_for_startup_turn_completion(provider: &JsProvider) -> bool {
    provider
        .manifest
        .spawn
        .public_send_waits_for_startup_turn_completion
        || provider
            .native()
            .is_some_and(|native| native.public_send_waits_for_startup_turn_completion())
}

pub(super) fn input_attach_signal(provider: &JsProvider) -> InputAttachSignal {
    match provider.manifest.spawn.input_attach {
        InputAttachSignal::None => provider.native().map_or(InputAttachSignal::None, |native| {
            native.input_attach_signal()
        }),
        declared => declared,
    }
}

pub(super) fn supports_unbound_submit_recovery(provider: &JsProvider) -> bool {
    provider
        .try_call::<bool>("supportsUnboundSubmitRecovery", &[])
        .unwrap_or_else(|| {
            provider
                .native()
                .is_some_and(|native| native.supports_unbound_submit_recovery())
        })
}
