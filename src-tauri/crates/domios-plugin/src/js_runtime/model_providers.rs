use super::vm::{call_json, PluginVm};
use crate::model_providers::{
    ConfigUpdate, ModelPreset, ModelProviderConfig, ModelProviderDefinition,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};

pub struct JsModelProviderConfig {
    vm: Arc<PluginVm>,
}

impl JsModelProviderConfig {
    pub fn new(vm: Arc<PluginVm>) -> Self {
        Self { vm }
    }
}

impl ModelProviderConfig for JsModelProviderConfig {
    fn read_presets(&self, config: &Value) -> Result<Vec<ModelPreset>, String> {
        call_json(&self.vm, "readModelPresets", &[config.clone()])
            .map_err(|_| "model presets unavailable".into())
    }
    fn apply_presets(
        &self,
        config: &Value,
        presets: &[ModelPreset],
    ) -> Result<ConfigUpdate, String> {
        call_json(
            &self.vm,
            "applyModelPresets",
            &[config.clone(), json!(presets)],
        )
        .map_err(|_| "model preset transform failed".into())
    }
    fn preset_launch_args(&self, name: &str) -> Result<Vec<String>, String> {
        call_json(&self.vm, "modelPresetLaunchArgs", &[json!(name)])
            .map_err(|_| "model preset launch unavailable".into())
    }
    fn path(&self) -> Result<PathBuf, String> {
        let path: String = call_json(&self.vm, "modelProviderConfigPath", &[])
            .map_err(|_| "model provider config path unavailable")?;
        if path.is_empty() || !self.vm.can_access_config_path(&path) {
            return Err("model provider config path unavailable".into());
        }
        Ok(PathBuf::from(path))
    }

    fn read(&self, config: &Value) -> Result<Vec<ModelProviderDefinition>, String> {
        call_json(&self.vm, "readModelProviders", &[config.clone()])
            .map_err(|_| "model provider config is invalid".into())
    }

    fn apply(
        &self,
        config: &Value,
        providers: &[ModelProviderDefinition],
    ) -> Result<ConfigUpdate, String> {
        call_json(
            &self.vm,
            "applyModelProviders",
            &[config.clone(), json!(providers)],
        )
        .map_err(|_| "model provider config transform failed".into())
    }

    fn key_env_name(&self, id: &str) -> Result<String, String> {
        call_json(&self.vm, "modelProviderKeyEnvName", &[json!(id)])
            .map_err(|_| "model provider environment name unavailable".into())
    }
}
