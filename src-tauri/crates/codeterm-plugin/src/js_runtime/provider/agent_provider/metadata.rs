macro_rules! js_provider_metadata {
    () => {
        fn account_capability(
            &self,
        ) -> Option<crate::provider_account::storage::AccountCapability> {
            self.manifest.accounts.clone()
        }

        fn general_agent_offer_when_absent(&self) -> bool {
            self.manifest.general_agent_offer_when_absent
        }

        fn supports_subagent_visibility(&self) -> bool {
            capabilities::supports_subagent_visibility(self)
        }

        fn attachment_capabilities(&self) -> crate::provider::AttachmentCapabilities {
            self.manifest.capabilities.attachments
        }
    };
}
