use super::*;
use crate::js_runtime::SerdeModelInfo;

impl JsProvider {
    pub(super) async fn js_discover_models(&self) -> Option<Vec<ModelInfo>> {
        if !self.has_js("discoverModels") {
            return None;
        }
        let result = crate::js_runtime::vm::with_blocking_operation_interruptible(
            self.vm.clone(),
            (
                crate::converter::ConversionCancellation::default(),
                std::time::Instant::now() + crate::model_catalog::MODEL_DISCOVERY_DEADLINE,
            ),
            |vm, context| {
                let _catalogue = vm.catalogue_gate.enter(&context)?;
                let fingerprint = vm.binary_fingerprint();
                crate::js_runtime::vm::call_in_operation::<Option<Vec<SerdeModelInfo>>>(
                    vm,
                    "discoverModels",
                    &[json!(fingerprint)],
                    context,
                )
            },
        )
        .await;
        match result {
            Ok(models) => models.map(|models| models.iter().map(ModelInfo::from).collect()),
            Err(error) => {
                log::warn!(
                    "[plugin {}] discoverModels failed: {error}",
                    self.manifest.id
                );
                None
            }
        }
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
