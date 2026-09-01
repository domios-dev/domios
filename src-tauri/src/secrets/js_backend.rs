use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use crate::secrets::{
    CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef, SecretStore,
    SecretString, SecretValue, StoreCapabilities, StoreStatus, UnlockCredentials,
};
use codeterm_plugin::js_runtime::{call_json, has_method, PluginVm};

pub struct JsSecretBackend {
    vm: Arc<PluginVm>,
    id: &'static str,
    capabilities: StoreCapabilities,
}

impl JsSecretBackend {
    pub fn new(vm: Arc<PluginVm>, id: &str, declares_auto_unlock: bool) -> Self {
        // once at construction (bounded: one small leak per plugin load).
        Self {
            vm,
            id: Box::leak(id.to_string().into_boxed_str()),
            capabilities: StoreCapabilities {
                auto_unlock: declares_auto_unlock,
            },
        }
    }

    fn call<R: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        args: &[serde_json::Value],
    ) -> Result<R, SecretError> {
        match call_json::<Envelope<R>>(&self.vm, method, args) {
            Ok(env) => env.into_result(),
            Err(e) => Err(SecretError::Backend {
                message: format!("plugin {method} failed: {e:?}"),
            }),
        }
    }

    fn call_unit(&self, method: &str, args: &[serde_json::Value]) -> Result<(), SecretError> {
        self.call::<serde_json::Value>(method, args).map(|_| ())
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
pub(crate) fn unlock_args(creds: &UnlockCredentials) -> serde_json::Value {
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
        match call_json::<StoreStatus>(&self.vm, "secretStatus", &[]) {
            Ok(s) => s,
            Err(e) => StoreStatus::Unavailable {
                reason: format!("status failed: {e:?}"),
            },
        }
    }

    async fn unlock(&self, creds: UnlockCredentials) -> Result<(), SecretError> {
        self.call_unit("secretUnlock", &[unlock_args(&creds)])
    }

    async fn lock(&self) -> Result<(), SecretError> {
        self.call_unit("secretLock", &[])
    }

    async fn logout(&self) -> Result<(), SecretError> {
        self.call_unit("secretLogout", &[])
    }

    async fn save(&self, req: SaveRequest) -> Result<SecretRef, SecretError> {
        let args = [json!({
            "name": req.name,
            "value": req.value.expose_secret(),
            "notes": req.notes,
            "folder": req.folder,
            "organization": req.organization,
            "collection": req.collection,
            "overwrite": req.overwrite,
        })];
        self.call::<SecretRef>("secretSave", &args)
    }

    async fn get(&self, name_or_id: &str) -> Result<SecretValue, SecretError> {
        let out = self.call::<GetOut>("secretGetItem", &[json!(name_or_id)])?;
        Ok(SecretValue {
            item_id: out.item_id,
            name: out.name,
            value: SecretString::from(out.value),
            notes: out.notes,
        })
    }

    async fn list(&self, filter: ListFilter) -> Result<Vec<SecretRef>, SecretError> {
        self.call::<Vec<SecretRef>>(
            "secretList",
            &[serde_json::to_value(&filter).unwrap_or(json!({}))],
        )
    }

    async fn search(&self, query: &str) -> Result<Vec<SecretRef>, SecretError> {
        self.call::<Vec<SecretRef>>("secretSearch", &[json!(query)])
    }

    async fn delete(&self, name_or_id: &str) -> Result<(), SecretError> {
        self.call_unit("secretDelete", &[json!(name_or_id)])
    }

    async fn organizations(&self) -> Result<Vec<OrganizationRef>, SecretError> {
        if !has_method(&self.vm, "secretOrganizations") {
            return Ok(Vec::new());
        }
        self.call::<Vec<OrganizationRef>>("secretOrganizations", &[])
    }

    async fn collections(
        &self,
        organization_id: Option<&str>,
    ) -> Result<Vec<CollectionRef>, SecretError> {
        if !has_method(&self.vm, "secretCollections") {
            return Ok(Vec::new());
        }
        self.call::<Vec<CollectionRef>>("secretCollections", &[json!(organization_id)])
    }

    async fn sync(&self) -> Result<(), SecretError> {
        if !has_method(&self.vm, "secretSync") {
            return Ok(());
        }
        self.call_unit("secretSync", &[])
    }

    async fn init(&self) -> Result<(), SecretError> {
        if !has_method(&self.vm, "secretInit") {
            return Ok(());
        }
        self.call_unit("secretInit", &[])
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
