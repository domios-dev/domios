use super::*;

impl JsProvider {
    pub(super) fn js_directory_trust_prompt_response(
        &self,
        screen: &str,
    ) -> Option<crate::provider::DirectoryTrustPromptResponse> {
        let steps = self
            .try_call::<Option<Vec<String>>>("directoryTrustPromptResponse", &[json!(screen)])??;
        use base64::Engine;
        let engine = base64::engine::general_purpose::STANDARD;
        let decoded = steps
            .iter()
            .map(|step| engine.decode(step.as_bytes()).ok())
            .collect::<Option<Vec<_>>>()
            .unwrap_or_default();
        Some(crate::provider::DirectoryTrustPromptResponse::from_wire_steps(decoded))
    }

    pub(super) fn js_pre_trust_directory(
        &self,
        path: &str,
    ) -> Result<crate::provider::PreTrustOutcome, String> {
        if self.has_js("preTrustDirectory") {
            return self
                .try_call("preTrustDirectory", &[json!(path)])
                .ok_or_else(|| "pre-trust capability failed".to_string());
        }
        self.native()
            .map(|native| native.pre_trust_directory(path))
            .unwrap_or_else(|| {
                Ok(crate::provider::PreTrustOutcome::Skipped(
                    "not supported".into(),
                ))
            })
    }

    pub(super) fn js_is_directory_trusted(&self, path: &str) -> Result<bool, String> {
        const METHOD: &str = "isDirectoryTrusted";
        if self.has_js(METHOD) {
            return super::super::call_json_strict(&self.vm, METHOD, &[json!(path)]).map_err(
                |error| format!("[plugin {}] {METHOD} failed: {error}", self.manifest.id),
            );
        }
        self.native()
            .map(|native| native.is_directory_trusted(path))
            .unwrap_or(Ok(false))
    }

    pub(super) fn js_directory_trust_state(
        &self,
        path: &str,
        skip_permissions: bool,
    ) -> Option<crate::provider::DirectoryTrustState> {
        if !self.has_js("directoryTrustState") {
            return self
                .native()
                .and_then(|native| native.directory_trust_state(path, skip_permissions));
        }
        Some(
            self.try_call::<crate::provider::DirectoryTrustState>(
                "directoryTrustState",
                &[json!(path), json!(skip_permissions)],
            )
            .unwrap_or(crate::provider::DirectoryTrustState::Unknown),
        )
    }
}
