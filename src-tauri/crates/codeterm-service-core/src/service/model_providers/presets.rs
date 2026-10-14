use super::*;
use codeterm_plugin::model_providers::ModelPreset;

fn validate(preset: &ModelPreset) -> Result<(), ModelProviderError> {
    if !valid_id(&preset.name)
        || preset.model.trim().is_empty()
        || preset.model.len() > 256
        || preset.model.chars().any(char::is_control)
        || preset.prompt.trim().is_empty()
        || preset.prompt.len() > 262144
        || preset
            .params
            .temperature
            .is_some_and(|n| !n.is_finite() || !(0.0..=2.0).contains(&n))
        || preset
            .params
            .top_p
            .is_some_and(|n| !n.is_finite() || !(0.0..=1.0).contains(&n))
        || preset.params.steps.is_some_and(|n| n == 0 || n > 10000)
    {
        return Err(ModelProviderError::InvalidRequest);
    }
    Ok(())
}

pub(super) fn execute(
    adapter: &dyn ModelProviderConfig,
    path: &Path,
    config: &Value,
    request: &ModelProviderRequest,
) -> Result<Value, ModelProviderError> {
    let mut presets = adapter
        .read_presets(config)
        .map_err(|_| ModelProviderError::Config)?;
    let result = match request {
        ModelProviderRequest::PresetList => return Ok(json!({"presets":presets})),
        ModelProviderRequest::PresetAdd { preset } => {
            validate(preset)?;
            presets.retain(|existing| existing.name != preset.name);
            presets.push(preset.clone());
            json!({"preset":preset})
        }
        ModelProviderRequest::PresetRemove { name } => {
            if !presets.iter().any(|preset| &preset.name == name) {
                return Err(ModelProviderError::NotFound);
            }
            presets.retain(|preset| &preset.name != name);
            json!({"removed":name})
        }
        _ => return Err(ModelProviderError::InvalidRequest),
    };
    let update = adapter
        .apply_presets(config, &presets)
        .map_err(|_| ModelProviderError::Config)?;
    if !update.conflicts.is_empty() {
        return Err(ModelProviderError::Conflict);
    }
    if update.changed {
        let bytes =
            serde_json::to_vec_pretty(&update.config).map_err(|_| ModelProviderError::Config)?;
        codeterm_util::atomic_write::write_atomic(path, bytes)
            .map_err(|_| ModelProviderError::Config)?;
    }
    Ok(result)
}

pub(crate) fn resolve(
    adapter: &dyn ModelProviderConfig,
    name: &str,
    model: Option<&str>,
    args: &[String],
) -> Result<(String, Vec<String>), ModelProviderError> {
    let path = adapter.path().map_err(|_| ModelProviderError::Config)?;
    let config = read_config(&path)?;
    let preset = adapter
        .read_presets(&config)
        .map_err(|_| ModelProviderError::Config)?
        .into_iter()
        .find(|preset| preset.name == name)
        .ok_or(ModelProviderError::NotFound)?;
    validate(&preset)?;
    if model.is_some_and(|model| model != preset.model)
        || args.iter().any(|arg| {
            matches!(arg.as_str(), "--agent" | "-m" | "--model")
                || arg.starts_with("--agent=")
                || arg.starts_with("--model=")
                || arg.starts_with("-m=")
        })
    {
        return Err(ModelProviderError::Conflict);
    }
    let launch_args = adapter
        .preset_launch_args(name)
        .map_err(|_| ModelProviderError::Config)?;
    if launch_args.is_empty() {
        return Err(ModelProviderError::Unsupported);
    }
    Ok((preset.model, launch_args))
}

impl ServiceContext {
    pub(crate) fn apply_model_preset(
        &self,
        provider: &str,
        params: &mut super::super::SpawnAgentParams,
    ) -> Result<(), String> {
        let Some(name) = params.preset.as_deref() else {
            return Ok(());
        };
        let adapter = self
            .agent_registry
            .with_provider(provider, |p| p.model_provider_config())
            .flatten();
        let Some(adapter) = adapter else {
            return Ok(());
        };
        let (model, args) = resolve(
            &*adapter,
            name,
            params.model.as_deref(),
            params.args.as_deref().unwrap_or_default(),
        )
        .map_err(|error| error.to_string())?;
        params.model = Some(model);
        params.args.get_or_insert_default().extend(args);
        params.preset = None;
        Ok(())
    }
}
