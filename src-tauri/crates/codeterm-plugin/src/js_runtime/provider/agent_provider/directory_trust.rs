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

    fn declared_trust_record(
        &self,
        cwd: &str,
    ) -> Result<Option<(std::path::PathBuf, String)>, String> {
        use crate::js_runtime::manifest::{DirectoryTrustRecord, PluginManifest};
        let Some(DirectoryTrustRecord::TomlFolders { path }) =
            &self.manifest.directory_trust_record
        else {
            return Ok(None);
        };
        let path = PluginManifest::expand_home(path).ok_or("trust record path unavailable")?;
        if !path.is_absolute() {
            return Err("trust record path must be absolute".into());
        }
        let provider_home = self
            .manifest
            .sessions
            .as_ref()
            .and_then(|sessions| PluginManifest::expand_home(&sessions.root_dir))
            .and_then(|root| root.parent().map(std::path::Path::to_path_buf))
            .ok_or("trust record requires a declared provider home")?;
        if path.parent() != Some(provider_home.as_path()) {
            return Err("trust record must be inside the declared provider home".into());
        }
        let cwd = std::fs::canonicalize(cwd).map_err(|error| error.to_string())?;
        let cwd = codeterm_paths::strip_verbatim_prefix(&cwd.to_string_lossy());
        Ok(Some((path, cwd)))
    }

    pub(super) fn js_pre_trust_directory(
        &self,
        path: &str,
    ) -> Result<crate::provider::PreTrustOutcome, String> {
        if let Some((record, cwd)) = self.declared_trust_record(path)? {
            return Ok(crate::toml_folder_trust::grant(&record, &cwd));
        }
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
        if let Some((record, cwd)) = self.declared_trust_record(path)? {
            return crate::toml_folder_trust::trusted(&record, &cwd);
        }
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
