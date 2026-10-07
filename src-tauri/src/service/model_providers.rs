use super::ServiceContext;
use crate::secrets::{SaveRequest, SecretError, SecretStoreState, SecretString, SecretValue};
use codeterm_plugin::{
    model_providers::{ModelProviderConfig, ModelProviderDefinition},
    AgentProvider,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    fs::{File, OpenOptions},
    path::Path,
    sync::Arc,
    time::Duration,
};

mod connection;
pub(crate) mod presets;
#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelProviderError {
    Unsupported,
    Ambiguous,
    InvalidRequest,
    Conflict,
    NotFound,
    Config,
    Busy,
    SecretLocked,
    SecretUnavailable,
    SecretOwnership,
    CleanupFailed,
    Timeout,
    Unauthorized,
    Connection,
    InvalidResponse,
}

impl std::fmt::Display for ModelProviderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "model provider operation failed ({})",
            serde_json::to_value(self)
                .ok()
                .and_then(|v| v.as_str().map(str::to_owned))
                .unwrap_or_default()
        )
    }
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelProviderRequest {
    PresetAdd {
        preset: codeterm_plugin::model_providers::ModelPreset,
    },
    PresetRemove {
        name: String,
    },
    PresetList,
    Add {
        provider: ModelProviderDefinition,
        #[serde(deserialize_with = "secret_from_wire")]
        value: SecretString,
    },
    AddFromSecret {
        provider: ModelProviderDefinition,
        source_secret: String,
    },
    Remove {
        name: String,
    },
    List,
    Test {
        name: String,
    },
}

fn secret_from_wire<'de, D: serde::Deserializer<'de>>(d: D) -> Result<SecretString, D::Error> {
    String::deserialize(d).map(SecretString::from)
}

pub fn select_owner(
    providers: &[Arc<dyn AgentProvider>],
) -> Result<Arc<dyn AgentProvider>, ModelProviderError> {
    let mut owners = providers
        .iter()
        .filter(|p| p.model_provider_config().is_some());
    let owner = owners
        .next()
        .cloned()
        .ok_or(ModelProviderError::Unsupported)?;
    if owners.next().is_some() {
        return Err(ModelProviderError::Ambiguous);
    }
    Ok(owner)
}

fn config_lock(path: &Path) -> Result<File, ModelProviderError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|_| ModelProviderError::Config)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(path.with_extension("domios.lock"))
        .map_err(|_| ModelProviderError::Config)?;
    file.try_lock().map_err(|_| ModelProviderError::Busy)?;
    Ok(file)
}

fn read_config(path: &Path) -> Result<Value, ModelProviderError> {
    match std::fs::read(path) {
        Ok(bytes) => serde_json::from_slice::<Value>(&bytes)
            .ok()
            .filter(Value::is_object)
            .ok_or(ModelProviderError::Config),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(_) => Err(ModelProviderError::Config),
    }
}

fn write_config(
    adapter: &dyn ModelProviderConfig,
    path: &Path,
    config: &Value,
    providers: &[ModelProviderDefinition],
) -> Result<(), ModelProviderError> {
    let update = adapter
        .apply(config, providers)
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
    Ok(())
}

fn secret_error(error: SecretError) -> ModelProviderError {
    match error {
        SecretError::Locked | SecretError::LoggedOut => ModelProviderError::SecretLocked,
        SecretError::AlreadyExists => ModelProviderError::Conflict,
        SecretError::NotFound => ModelProviderError::NotFound,
        _ => ModelProviderError::SecretUnavailable,
    }
}

fn ownership(owner: &str, id: &str) -> String {
    format!("domios:model-provider:{owner}:{id}")
}

fn get_secret(
    store: &SecretStoreState,
    owner: &str,
    provider: &ModelProviderDefinition,
) -> Result<SecretValue, ModelProviderError> {
    let id = provider
        .secret_name
        .as_deref()
        .ok_or(ModelProviderError::SecretOwnership)?;
    let secret = tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            store.read().await.get(id).await
        })
        .await
    })
    .map_err(|_| ModelProviderError::Timeout)?
    .map_err(secret_error)?;
    if secret.name != id || secret.notes.as_deref() != Some(ownership(owner, &provider.id).as_str())
    {
        return Err(ModelProviderError::SecretOwnership);
    }
    Ok(secret)
}

fn delete_secret(store: &SecretStoreState, id: &str) -> Result<(), ModelProviderError> {
    tauri::async_runtime::block_on(async {
        tokio::time::timeout(Duration::from_secs(10), async {
            store.read().await.delete(id).await
        })
        .await
    })
    .map_err(|_| ModelProviderError::Timeout)?
    .map_err(secret_error)
}

fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id.as_bytes()[0].is_ascii_lowercase()
        && id.split('-').all(|part| {
            !part.is_empty()
                && part
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        })
}

fn env_name(adapter: &dyn ModelProviderConfig, id: &str) -> Result<String, ModelProviderError> {
    let name = adapter
        .key_env_name(id)
        .map_err(|_| ModelProviderError::Config)?;
    if !name.starts_with("DOMIOS_PROVIDER_")
        || !name.ends_with("_KEY")
        || !name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
    {
        return Err(ModelProviderError::Config);
    }
    Ok(name)
}

pub fn execute(
    store: &SecretStoreState,
    owner: &str,
    adapter: &dyn ModelProviderConfig,
    request: ModelProviderRequest,
) -> Result<Value, ModelProviderError> {
    let request = match request {
        ModelProviderRequest::AddFromSecret {
            mut provider,
            source_secret,
        } => {
            if !valid_id(&provider.id) || source_secret.trim().is_empty() {
                return Err(ModelProviderError::InvalidRequest);
            }
            let source = tauri::async_runtime::block_on(async {
                tokio::time::timeout(Duration::from_secs(10), async {
                    store.read().await.get(&source_secret).await
                })
                .await
            })
            .map_err(|_| ModelProviderError::Timeout)?
            .map_err(secret_error)?;
            provider.secret_name = Some(format!("domios/model-provider/{}", provider.id));
            ModelProviderRequest::Add {
                provider,
                value: source.value,
            }
        }
        request => request,
    };
    let path = adapter.path().map_err(|_| ModelProviderError::Config)?;
    let _lock = config_lock(&path)?;
    let config = read_config(&path)?;
    if matches!(
        &request,
        ModelProviderRequest::PresetAdd { .. }
            | ModelProviderRequest::PresetRemove { .. }
            | ModelProviderRequest::PresetList
    ) {
        return presets::execute(adapter, &path, &config, &request);
    }
    let mut providers = adapter
        .read(&config)
        .map_err(|_| ModelProviderError::Config)?;
    match request {
        ModelProviderRequest::AddFromSecret { .. }
        | ModelProviderRequest::PresetAdd { .. }
        | ModelProviderRequest::PresetRemove { .. }
        | ModelProviderRequest::PresetList => unreachable!(),
        ModelProviderRequest::List => Ok(json!({ "providers": providers })),
        ModelProviderRequest::Test { name } => {
            let provider = providers
                .iter()
                .find(|p| p.id == name)
                .ok_or(ModelProviderError::NotFound)?;
            let key = get_secret(store, owner, provider)?;
            let models = connection::test(provider, &key.value)?;
            Ok(json!({ "models": models }))
        }
        ModelProviderRequest::Add {
            mut provider,
            value,
        } => {
            if !valid_id(&provider.id)
                || value.is_empty()
                || value.len() > 65536
                || provider
                    .secret_name
                    .as_ref()
                    .is_none_or(|name| name.trim().is_empty())
                || provider.key_env.is_some()
            {
                return Err(ModelProviderError::InvalidRequest);
            }
            connection::validate_url(&provider.base_url)?;
            if providers.iter().any(|p| p.id == provider.id) {
                return Err(ModelProviderError::Conflict);
            }
            provider.key_env = Some(env_name(adapter, &provider.id)?);
            if providers.iter().any(|p| p.key_env == provider.key_env) {
                return Err(ModelProviderError::Conflict);
            }
            if provider.models.is_empty() {
                provider.models = connection::test(&provider, &value)?;
            }
            if provider
                .models
                .iter()
                .any(|m| m.is_empty() || m.len() > 256 || m.chars().any(char::is_control))
            {
                return Err(ModelProviderError::InvalidRequest);
            }
            providers.push(provider.clone());
            let preview = adapter
                .apply(&config, &providers)
                .map_err(|_| ModelProviderError::Config)?;
            if !preview.conflicts.is_empty() {
                return Err(ModelProviderError::Conflict);
            }
            let saved = tauri::async_runtime::block_on(async {
                tokio::time::timeout(Duration::from_secs(10), async {
                    store
                        .read()
                        .await
                        .save(SaveRequest {
                            name: provider.secret_name.clone().unwrap_or_default(),
                            value,
                            notes: Some(ownership(owner, &provider.id)),
                            folder: None,
                            organization: None,
                            collection: None,
                            overwrite: false,
                        })
                        .await
                })
                .await
            })
            .map_err(|_| ModelProviderError::Timeout)?
            .map_err(secret_error)?;
            if let Err(error) = write_config(adapter, &path, &config, &providers) {
                delete_secret(store, &saved.item_id)
                    .map_err(|_| ModelProviderError::CleanupFailed)?;
                return Err(error);
            }
            Ok(json!({ "provider": provider }))
        }
        ModelProviderRequest::Remove { name } => {
            let provider = providers
                .iter()
                .find(|p| p.id == name)
                .cloned()
                .ok_or(ModelProviderError::NotFound)?;
            let secret = if provider.secret_name.is_some() {
                match get_secret(store, owner, &provider) {
                    Ok(secret) => Some(secret),
                    Err(ModelProviderError::NotFound) => None,
                    Err(error) => return Err(error),
                }
            } else {
                None
            };
            providers.retain(|p| p.id != name);
            write_config(adapter, &path, &config, &providers)?;
            if secret.is_some_and(|secret| delete_secret(store, &secret.item_id).is_err()) {
                providers.push(provider);
                let current = read_config(&path)?;
                write_config(adapter, &path, &current, &providers)
                    .map_err(|_| ModelProviderError::CleanupFailed)?;
                return Err(ModelProviderError::SecretUnavailable);
            }
            Ok(json!({ "removed": name }))
        }
    }
}

impl ServiceContext {
    pub fn model_provider_command(
        &self,
        request: ModelProviderRequest,
    ) -> Result<Value, ModelProviderError> {
        let owner = select_owner(&self.agent_registry.all_provider_arcs())?;
        let adapter = owner
            .model_provider_config()
            .ok_or(ModelProviderError::Unsupported)?;
        let changes = matches!(
            &request,
            ModelProviderRequest::PresetAdd { .. }
                | ModelProviderRequest::PresetRemove { .. }
                | ModelProviderRequest::Add { .. }
                | ModelProviderRequest::AddFromSecret { .. }
                | ModelProviderRequest::Remove { .. }
        );
        let preset_list = matches!(&request, ModelProviderRequest::PresetList);
        let mut result = execute(&self.secrets, owner.id(), &*adapter, request)?;
        if preset_list {
            result["provider"] = json!(owner.id());
        }
        if changes {
            crate::agent::model_catalog::invalidate(owner.id());
            tauri::async_runtime::spawn(async move {
                let _ = crate::agent::model_catalog::refresh_models(owner, true).await;
            });
        }
        Ok(result)
    }

    pub(crate) fn link_provider_launch_pty(
        &self,
        claim: &super::pty_ownership::TabPtyClaim,
        tab_id: &str,
        candidate: String,
        displaced: Option<&str>,
        fresh_pty: bool,
    ) -> super::pty_ownership::PtySelection {
        if fresh_pty {
            if let Some(previous) = displaced {
                self.pty_ownership.release_pty(previous);
            }
        }
        self.link_pty_with_claim(claim, tab_id, candidate, true)
    }
    pub(crate) fn model_provider_requires_fresh_pty(&self, provider_id: &str) -> bool {
        self.agent_registry
            .provider_arc(provider_id)
            .is_some_and(|provider| requires_fresh_pty(&*provider))
    }
    pub(crate) fn model_provider_launch_env(
        &self,
        provider_id: &str,
    ) -> Result<HashMap<String, String>, ModelProviderError> {
        let Some(adapter) = self
            .agent_registry
            .provider_arc(provider_id)
            .and_then(|p| p.model_provider_config())
        else {
            return Ok(HashMap::new());
        };
        let path = adapter.path().map_err(|_| ModelProviderError::Config)?;
        let providers = adapter
            .read(&read_config(&path)?)
            .map_err(|_| ModelProviderError::Config)?;
        launch_env(&self.secrets, provider_id, &*adapter, &providers)
    }
}

fn launch_env(
    store: &SecretStoreState,
    owner: &str,
    adapter: &dyn ModelProviderConfig,
    providers: &[ModelProviderDefinition],
) -> Result<HashMap<String, String>, ModelProviderError> {
    let mut env = HashMap::new();
    for provider in providers.iter().filter(|p| p.secret_name.is_some()) {
        let name = env_name(adapter, &provider.id)?;
        if provider.key_env.as_deref() != Some(&name) || env.contains_key(&name) {
            return Err(ModelProviderError::Config);
        }
        let secret = get_secret(store, owner, provider)?;
        env.insert(name, secret.value.into_inner());
    }
    Ok(env)
}

fn requires_fresh_pty(provider: &dyn AgentProvider) -> bool {
    provider.model_provider_config().is_some()
}
