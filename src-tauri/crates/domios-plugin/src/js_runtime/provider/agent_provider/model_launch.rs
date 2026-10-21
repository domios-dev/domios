use super::*;
use crate::js_runtime::SerdeModelInfo;

impl JsProvider {
    pub(super) fn js_discover_models(&self) -> Option<Vec<ModelInfo>> {
        if !self.has_js("discoverModels") {
            return None;
        }
        let _catalogue = self.vm.catalogue_gate.lock();
        let fingerprint = self.vm.binary_fingerprint();
        // `null` is the plugin's honest "no genuine discovery" signal — it must
        // not deserialize-fail into warn noise, and never falls back to a seed.
        self.try_call::<Option<Vec<SerdeModelInfo>>>("discoverModels", &[json!(fingerprint)])
            .flatten()
            .map(|models| models.iter().map(ModelInfo::from).collect())
    }

    pub(super) fn js_model_launch_args(&self, model: &str) -> Vec<String> {
        if model.is_empty() {
            return Vec::new();
        }
        let tmpl = &self.manifest.commands.model_launch_args_format;
        if tmpl.is_empty() {
            return vec!["--model".into(), model.into()];
        }
        tmpl.iter().map(|t| t.replace("{model}", model)).collect()
    }

    pub(super) fn js_reasoning_effort_launch_args(&self, effort: &str) -> Vec<String> {
        if effort.is_empty() {
            return Vec::new();
        }
        self.manifest
            .commands
            .reasoning_effort_launch_args_format
            .iter()
            .map(|t| t.replace("{effort}", effort))
            .collect()
    }

    pub(super) fn js_reasoning_effort_launch_args_for_model(
        &self,
        model: Option<&str>,
        effort: &str,
    ) -> Vec<String> {
        self.try_call(
            "reasoningEffortLaunchArgsForModel",
            &[json!(model), json!(effort)],
        )
        .unwrap_or_else(|| self.reasoning_effort_launch_args(effort))
    }
}
