use super::JsProvider;
use crate::provider_self_update::ProviderSelfUpdateCapability;

impl JsProvider {
    pub(super) fn js_self_update_capability(&self) -> Option<ProviderSelfUpdateCapability> {
        if self.has_js("providerSelfUpdateCapability") {
            self.try_call::<Option<ProviderSelfUpdateCapability>>(
                "providerSelfUpdateCapability",
                &[],
            )
            .flatten()
        } else {
            self.native()
                .and_then(|native| native.provider_self_update_capability())
        }
    }
}
