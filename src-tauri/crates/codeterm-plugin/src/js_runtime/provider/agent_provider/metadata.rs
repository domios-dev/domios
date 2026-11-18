macro_rules! js_provider_metadata {
    () => {
        fn account_implementation_revision(&self) -> Option<String> {
            Some(self.manifest.version.clone())
        }

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

        fn named_account_session_hook(
            &self,
            home: &std::path::Path,
            repair_consented: bool,
        ) -> Result<Option<bool>, String> {
            match self
                .native()
                .filter(|provider| provider.integration_kind() == self.integration_kind())
            {
                Some(native) => native.named_account_session_hook(home, repair_consented),
                None => Ok(None),
            }
        }

        fn uninstall_named_account_integration(
            &self,
            home: &std::path::Path,
        ) -> Result<(), String> {
            self.native()
                .filter(|provider| provider.integration_kind() == self.integration_kind())
                .map_or(Ok(()), |native| {
                    native.uninstall_named_account_integration(home)
                })
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

        fn model_switch(&self) -> crate::provider::ModelSwitchMode {
            self.manifest.commands.model_switch
        }

        fn model_switch_commands(&self) -> Option<crate::provider::ModelSwitchCommands> {
            self.manifest.commands.model_switch_commands.clone()
        }

        fn model_switch_terminal_label(&self, model: &str) -> Option<String> {
            self.manifest
                .models
                .iter()
                .find(|entry| entry.id == model)
                .map(|entry| entry.display_name.clone())
        }
    };
}
