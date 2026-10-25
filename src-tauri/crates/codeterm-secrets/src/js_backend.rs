use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::{
    CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef, SecretStore,
    SecretString, SecretValue, StoreCapabilities, StoreStatus, UnlockCredentials,
};
use codeterm_plugin::js_runtime::{call_json, has_method, PluginVm, VmError};

/// The plugin entry points a secret backend calls. A call may block for as long
/// as the vault CLI takes, so it never runs on an async worker.
pub trait PluginCalls: Send + Sync + 'static {
    fn has_method(&self, method: &str) -> bool;
    fn call(&self, method: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, VmError>;
}

impl PluginCalls for PluginVm {
    fn has_method(&self, method: &str) -> bool {
        has_method(self, method)
    }

    fn call(&self, method: &str, args: &[serde_json::Value]) -> Result<serde_json::Value, VmError> {
        call_json(self, method, args)
    }
}

pub struct JsSecretBackend {
    plugin: Arc<dyn PluginCalls>,
    id: &'static str,
    capabilities: StoreCapabilities,
}

impl JsSecretBackend {
    pub fn new(plugin: Arc<dyn PluginCalls>, id: &str, declares_auto_unlock: bool) -> Self {
        // once at construction (bounded: one small leak per plugin load).
        Self {
            plugin,
            id: Box::leak(id.to_string().into_boxed_str()),
            capabilities: StoreCapabilities {
                auto_unlock: declares_auto_unlock,
            },
        }
    }

    async fn call_raw(
        &self,
        method: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, SecretError> {
        let plugin = self.plugin.clone();
        tokio::task::spawn_blocking(move || plugin.call(method, &args))
            .await
            .map_err(|_| SecretError::Backend {
                message: format!("plugin {method} worker failed"),
            })?
            .map_err(|e| SecretError::Backend {
                message: format!("plugin {method} failed ({})", vm_error_kind(&e)),
            })
    }

    async fn call_value(
        &self,
        method: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<serde_json::Value, SecretError> {
        let value = self.call_raw(method, args).await?;
        serde_json::from_value::<Envelope<serde_json::Value>>(value)
            .map_err(|_| unexpected_shape(method))?
            .into_result()
            .map_err(SecretError::redacted)
    }

    async fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<R, SecretError> {
        let value = self.call_value(method, args).await?;
        serde_json::from_value::<R>(value).map_err(|_| unexpected_shape(method))
    }

    async fn call_metadata(
        &self,
        method: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<SecretRef>, SecretError> {
        crate::redact::metadata_list(self.call_value(method, args).await?)
            .map_err(|_| unexpected_shape(method))
    }

    async fn call_unit(
        &self,
        method: &'static str,
        args: Vec<serde_json::Value>,
    ) -> Result<(), SecretError> {
        self.call::<serde_json::Value>(method, args)
            .await
            .map(|_| ())
    }
}

fn vm_error_kind(e: &VmError) -> &'static str {
    match e {
        VmError::ReadFile { .. } => "read_file",
        VmError::RuntimeInit(_) => "runtime_init",
        VmError::Script(_) => "script",
        VmError::MissingFunction(_) => "missing_function",
        VmError::InvalidJson(..) => "invalid_json",
        VmError::ArgSerialize(..) => "arg_serialize",
        VmError::AsyncJobTimedOut => "timed_out",
    }
}

fn unexpected_shape(method: &str) -> SecretError {
    SecretError::Backend {
        message: format!("plugin {method} returned an unexpected response shape"),
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    #[serde(default = "none")]
    ok: Option<T>,
    #[serde(default)]
    error: Option<SecretError>,
}

fn none<T>() -> Option<T> {
    None
}

impl<T> Envelope<T> {
    fn into_result(self) -> Result<T, SecretError> {
        if let Some(e) = self.error {
            return Err(e);
        }
        self.ok.ok_or(SecretError::Backend {
            message: "plugin returned neither ok nor error".into(),
        })
    }
}

#[derive(Deserialize)]
struct GetOut {
    item_id: String,
    name: String,
    value: String,
    #[serde(default)]
    notes: Option<String>,
}

/// The `secretUnlock` argument. `persistForAutoUnlock` is always serialized as a
/// bool so a drop-in never infers consent from an absent field.
pub fn unlock_args(creds: &UnlockCredentials) -> serde_json::Value {
    json!({
        "masterPassword": creds.master_password.expose_secret(),
        "twoFactorToken": creds.two_factor_token.as_ref().map(|t| t.expose_secret()),
        "email": creds.email,
        "apiKeyClientId": creds.api_key_client_id.as_ref().map(|t| t.expose_secret()),
        "apiKeyClientSecret": creds.api_key_client_secret.as_ref().map(|t| t.expose_secret()),
        "persistForAutoUnlock": creds.persist_for_auto_unlock,
    })
}

#[async_trait]
impl SecretStore for JsSecretBackend {
    fn id(&self) -> &'static str {
        self.id
    }

    fn capabilities(&self) -> StoreCapabilities {
        self.capabilities
    }

    async fn status(&self) -> StoreStatus {
        let status = match self.call_raw("secretStatus", Vec::new()).await {
            Ok(value) => serde_json::from_value::<StoreStatus>(value)
                .map_err(|_| unexpected_shape("secretStatus")),
            Err(e) => Err(e),
        };
        match status {
            Ok(StoreStatus::Unavailable { reason }) => StoreStatus::Unavailable {
                reason: crate::redact::bounded_reason(&reason),
            },
            Ok(other) => other,
            Err(e) => StoreStatus::Unavailable {
                reason: format!("status failed: {e}"),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.call_unit("secretUnlock", vec![unlock_args(&creds)])
            .await
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.call_unit("secretLock", Vec::new()).await
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.call_unit("secretLogout", Vec::new()).await
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        let args = vec![json!({
            "name": req.name,
            "value": req.value.expose_secret(),
            "notes": req.notes,
            "folder": req.folder,
            "organization": req.organization,
            "collection": req.collection,
            "overwrite": req.overwrite,
        })];
        let saved = self.call_value("secretSave", args).await?;
        crate::redact::metadata_only(&saved).ok_or_else(|| unexpected_shape("secretSave"))
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        let out = self
            .call::<GetOut>("secretGetItem", vec![json!(name_or_id)])
            .await?;
        Ok(SecretValue {
            item_id: out.item_id,
            name: out.name,
            value: SecretString::from(out.value),
            notes: out.notes,
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.call_metadata(
            "secretList",
            vec![serde_json::to_value(&filter).unwrap_or(json!({}))],
        )
        .await
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.call_metadata("secretSearch", vec![json!(query)]).await
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.call_unit("secretDelete", vec![json!(name_or_id)])
            .await
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        if !self.plugin.has_method("secretOrganizations") {
            return Ok(Vec::new());
        }
        self.call::<Vec<OrganizationRef>>("secretOrganizations", Vec::new())
            .await
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        if !self.plugin.has_method("secretCollections") {
            return Ok(Vec::new());
        }
        self.call::<Vec<CollectionRef>>("secretCollections", vec![json!(organization_id)])
            .await
    }

    async fn sync(&self) -> Result<(), SecretError> {
        if !self.plugin.has_method("secretSync") {
            return Ok(());
        }
        self.call_unit("secretSync", Vec::new()).await
    }

    async fn init(&self) -> Result<(), SecretError> {
        if !self.plugin.has_method("secretInit") {
            return Ok(());
        }
        self.call_unit("secretInit", Vec::new()).await
    }
}

pub struct AppSecretBackendRegistrar {
    registry: std::sync::Arc<super::SecretRegistry>,
}

impl AppSecretBackendRegistrar {
    pub fn new(registry: std::sync::Arc<super::SecretRegistry>) -> Self {
        Self { registry }
    }
}

impl codeterm_plugin::SecretBackendRegistrar for AppSecretBackendRegistrar {
    fn register_secret_backend(
        &self,
        plugin_id: String,
        vm: std::sync::Arc<codeterm_plugin::js_runtime::PluginVm>,
        declares_auto_unlock: bool,
    ) {
        self.registry.register(
            plugin_id.clone(),
            std::sync::Arc::new(JsSecretBackend::new(vm, &plugin_id, declares_auto_unlock)),
        );
    }

    fn unregister_secret_backend(&self, plugin_id: &str) {
        self.registry.unregister(plugin_id);
    }
}

#[cfg(test)]
mod tests;
