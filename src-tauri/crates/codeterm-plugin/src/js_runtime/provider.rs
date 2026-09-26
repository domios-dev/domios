use std::sync::Arc;

use serde::Deserialize;
use serde_json::{json, Value};

mod agent_provider;
mod command_template;
mod launch_artifacts;
mod manifest_sessions;
mod prompt_parse;
mod wire;

use wire::{result_from_js, SerdeSessionMessage, VmUsageParser};

use crate::provider::{
    self, AgentProvider, ModelInfo, ProviderTrust, ReasoningEffortInfo, SessionMessage,
    SettingField,
};

use super::{call_json, PluginManifest, PluginVm, VmError};

/// One generic provider per plugin bundle.
pub struct JsProvider {
    vm: Arc<PluginVm>,
    manifest: PluginManifest,
    trust: ProviderTrust,
    /// Registry handle for lazy native-fallback lookup. Capability methods the
    /// JS plugin does NOT export delegate to the native provider of the same id
    /// (displaced when this JS plugin took authority) — so a stub JS plugin
    /// (e.g. opencode, no sqlite in JS) still gets the native implementation.
    /// Looked up lazily so it survives hot-reload re-registration. Generic: any
    /// provider source (json/sql/…) works as long as something implements the
    /// trait method.
    registry: Option<Arc<dyn crate::host_traits::PluginProviderRegistrar>>,
    /// settingsSchema/notificationsSchema are pure constants in the plugin but
    /// `describe()` (called on every /api/agents/providers poll) re-evaluated
    /// them through QuickJS every time — a profiled CPU hot path. Compute once.
    schema_cache: std::sync::OnceLock<(Vec<SettingField>, Vec<SettingField>)>,
    has_custom_settings_schema: bool,
    /// VM-backed usage parser, built once when the plugin exports `parseUsage`.
    /// `usage_parser()` hands out a reference, so the adapter must live on the
    /// provider.
    usage_parser_cell: std::sync::OnceLock<Option<VmUsageParser>>,
}

impl JsProvider {
    pub fn new(manifest: PluginManifest, vm: Arc<PluginVm>) -> Self {
        Self::new_with_trust(manifest, vm, ProviderTrust::ThirdParty)
    }

    pub fn new_with_trust(
        manifest: PluginManifest,
        vm: Arc<PluginVm>,
        trust: ProviderTrust,
    ) -> Self {
        // Register the plugin's models with the catalog so settings UI and `default_model()` resolve through the same plumbing as native providers.
        if !manifest.models.is_empty() {
            crate::model_catalog::register(
                &manifest.id,
                crate::model_catalog::RegisteredDefaults {
                    models: manifest.models.iter().map(ModelInfo::from).collect(),
                    default: manifest.default_model.clone(),
                },
            );
        }
        crate::model_catalog::register_reasoning_efforts(
            &manifest.id,
            manifest
                .reasoning_efforts
                .iter()
                .map(ReasoningEffortInfo::from)
                .collect(),
            manifest.default_reasoning_effort.clone(),
        );
        let has_custom_settings_schema = super::vm::has_method(&vm, "settingsSchema");
        Self {
            vm,
            manifest,
            trust,
            registry: None,
            schema_cache: std::sync::OnceLock::new(),
            has_custom_settings_schema,
            usage_parser_cell: std::sync::OnceLock::new(),
        }
    }

    /// Evaluate the plugin's (settings, notifications) schemas once via QuickJS.
    fn compute_schemas(&self) -> (Vec<SettingField>, Vec<SettingField>) {
        let settings = self
            .try_call::<Vec<SettingField>>("settingsSchema", &[])
            .unwrap_or_else(|| manifest_settings_sections(&self.manifest));
        let notifications = self
            .try_call::<Vec<SettingField>>("notificationsSchema", &[])
            .unwrap_or_default();
        (settings, notifications)
    }

    /// Give the provider a registry handle so it can look up its native
    /// fallback for unexported capability methods. Set by the loader/watcher.
    pub fn set_registrar(
        &mut self,
        registry: Arc<dyn crate::host_traits::PluginProviderRegistrar>,
    ) {
        self.registry = Some(registry);
    }

    /// Whether the JS bundle exports `method` — distinguishes "not implemented"
    /// (delegate to native) from "implemented, returned empty".
    fn has_js(&self, method: &str) -> bool {
        super::vm::has_method(&self.vm, method)
    }

    /// The native provider of this id displaced by this JS plugin, if any.
    fn native(&self) -> Option<Arc<dyn AgentProvider>> {
        self.registry.as_ref()?.native_fallback(&self.manifest.id)
    }

    /// Manifest-declared hook roster resolved against the closed route set.
    /// `None` = nothing declared; `Err` = declared but invalid (fails closed).
    fn declared_hook_event_roster(
        &self,
    ) -> Option<Result<Vec<provider::HookEventBinding>, String>> {
        self.manifest
            .spawn
            .hook_event_roster
            .as_deref()
            .map(super::resolve_hook_event_roster)
    }

    pub fn manifest(&self) -> &PluginManifest {
        &self.manifest
    }

    pub fn vm(&self) -> &Arc<PluginVm> {
        &self.vm
    }

    /// Try to call a plugin method, returning `None` if it isn't exported
    /// and logging anything else. Swallowing missing-function lets plugin
    /// authors omit methods they don't implement.
    fn try_call<R: for<'de> Deserialize<'de>>(&self, method: &str, args: &[Value]) -> Option<R> {
        match call_json::<R>(&self.vm, method, args) {
            Ok(v) => Some(v),
            Err(VmError::MissingFunction(_)) => None,
            Err(VmError::Script(msg)) if msg.contains("not a function") => None,
            Err(e) => {
                log::warn!("[plugin {}] {} failed: {e}", self.manifest.id, method);
                None
            }
        }
    }

    fn extend_parsed_session_chunk(&self, out: &mut Vec<SessionMessage>, chunk: &str) {
        if let Some(parsed) =
            self.try_call::<Vec<SerdeSessionMessage>>("parseSessionMessages", &[json!(chunk)])
        {
            out.extend(parsed.into_iter().map(SessionMessage::from));
        }
    }

    /// Call a plugin method expected to return `{ok: bool, error?: string}`.
    /// Returns `None` when the plugin doesn't export the method (so the
    /// caller can fall back to a manifest-driven default); returns
    /// `Some(Ok)` / `Some(Err)` once the plugin replies.
    fn try_call_result(&self, method: &str, args: &[Value]) -> Option<Result<(), String>> {
        self.try_call::<serde_json::Value>(method, args)
            .map(|v| result_from_js(&v))
    }
}

#[cfg(test)]
mod tests;
