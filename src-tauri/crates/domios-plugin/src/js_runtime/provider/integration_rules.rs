use super::super::PluginManifest;
use super::JsProvider;
use serde_json::json;

impl JsProvider {
    pub(super) fn install_bridge_rule_from_manifest(&self, payload: &str) -> Result<(), String> {
        // manifest.rule.path when present — keeps the contract honest for plugins that declared the path but skipped the JS.
        if let Some(r) = self.try_call_result("installBridgeRule", &[json!(payload)]) {
            return r;
        }
        let Some(rule) = self.manifest.rule.as_ref() else {
            return Ok(());
        };
        let path = PluginManifest::expand_home(&rule.path)
            .ok_or_else(|| "rule.path: home dir not resolvable".to_string())?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        std::fs::write(&path, payload).map_err(|e| format!("write {}: {e}", path.display()))
    }

    pub(super) fn uninstall_bridge_rule_from_manifest(&self) -> Result<(), String> {
        if let Some(r) = self.try_call_result("uninstallBridgeRule", &[]) {
            return r;
        }
        let Some(rule) = self.manifest.rule.as_ref() else {
            return Ok(());
        };
        let Some(path) = PluginManifest::expand_home(&rule.path) else {
            return Ok(());
        };
        if !path.exists() {
            return Ok(());
        }
        std::fs::remove_file(&path).map_err(|e| format!("remove {}: {e}", path.display()))
    }
}
