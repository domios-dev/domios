use super::*;

impl JsProvider {
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
