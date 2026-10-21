use super::super::IntegrationKind;
use super::JsProvider;
use crate::provider;
use serde_json::json;

impl JsProvider {
    pub(super) fn js_check_integration(&self) -> serde_json::Value {
        if let Some(Err(error)) = self.declared_hook_event_roster() {
            return serde_json::json!({
                "status": "error",
                "hooks_installed": false,
                "hook_event_roster_error": error,
            });
        }
        self.try_call::<serde_json::Value>("checkIntegration", &[])
            .unwrap_or_else(|| serde_json::json!({ "status": "ok" }))
    }

    pub(super) fn js_integration_kind(&self) -> IntegrationKind {
        self.manifest
            .integration
            .as_ref()
            .map_or(IntegrationKind::None, |integration| integration.kind)
    }

    pub(super) fn js_permission_hook_capability(
        &self,
    ) -> Option<provider::PermissionHookCapability> {
        self.try_call("permissionHookCapability", &[])
    }

    pub(super) fn js_install_integration(
        &self,
        api_port: u16,
        options: &crate::provider::IntegrationInstallOptions,
    ) -> Result<(), String> {
        if !options.consent {
            return Err(
                "provider integration installation requires explicit local-user consent".into(),
            );
        }
        if let Some(Err(error)) = self.declared_hook_event_roster() {
            return Err(error);
        }
        self.try_call_result("installIntegration", &[json!(api_port), json!(options)])
            .unwrap_or(Ok(()))
    }

    pub(super) fn js_uninstall_integration(&self) -> Result<(), String> {
        self.try_call_result("uninstallIntegration", &[])
            .unwrap_or(Ok(()))
    }

    pub(super) fn js_restore_integration(&self) -> Result<(), String> {
        self.try_call_result("restoreIntegration", &[])
            .unwrap_or_else(|| Err("provider integration has no recoverable backup".into()))
    }

    pub(super) fn js_integration_trust_grant_capability(
        &self,
    ) -> Option<crate::provider::IntegrationTrustGrantCapability> {
        self.manifest
            .integration
            .as_ref()?
            .trust_grant
            .as_ref()
            .map(|grant| crate::provider::IntegrationTrustGrantCapability {
                managed_keys: grant.managed_keys.clone(),
            })
    }

    pub(super) fn js_grant_integration_trust(&self, managed_keys: &[String]) -> serde_json::Value {
        self.try_call::<serde_json::Value>("grantIntegrationTrust", &[json!(managed_keys)])
            .unwrap_or_else(|| {
                serde_json::json!({
                    "granted": false,
                    "degraded": true,
                    "reason": "plugin exports no grantIntegrationTrust"
                })
            })
    }

    pub(super) fn js_ensure_integration_on_startup(&self, api_port: u16) {
        let _ =
            self.try_call::<serde_json::Value>("ensureIntegrationOnStartup", &[json!(api_port)]);
    }
}
