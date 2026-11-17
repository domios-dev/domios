macro_rules! js_provider_metadata {
    () => {
        fn account_identity(
            &self,
            record: Option<&Value>,
        ) -> Result<Option<crate::provider_account::storage::AccountIdentity>, String> {
            if !self.has_js("accountIdentity") {
                return self
                    .native()
                    .map(|provider| provider.account_identity(record))
                    .unwrap_or(Ok(None));
            }
            let identity: Option<crate::provider_account::storage::AccountIdentity> = call_json(
                &self.vm,
                "accountIdentity",
                &[record.cloned().unwrap_or(Value::Null)],
            )
            .map_err(|_| "The harness account identity could not be read".to_owned())?;
            if let Some(identity) = &identity {
                identity.validate()?;
            }
            Ok(identity)
        }

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
