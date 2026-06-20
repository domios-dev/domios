//! `JsSecretBackend` — adapts a JS `capabilities.secret_backend` plugin to the core
//! [`SecretStore`](crate::secrets::SecretStore) trait via the synchronous `call_json`.
//! Wire contract: methods return `{ ok }` / `{ error: { kind, ... } }`; `secretStatus`
//! returns a bare `StoreStatus`; unexported optional methods fall back to the trait default.

use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use super::vm::{call_json, has_method, PluginVm};
use crate::secrets::{
    CollectionRef, ListFilter, OrganizationRef, SaveRequest, SecretError, SecretRef, SecretStore,
    SecretString, SecretValue, StoreStatus, UnlockCredentials,
};

pub struct JsSecretBackend {
    vm: Arc<PluginVm>,
    id: &'static str,
}

impl JsSecretBackend {
    pub fn new(vm: Arc<PluginVm>, id: &str) -> Self {
        // SecretStore::id is `&'static str`; the plugin id is dynamic, so leak it
        // once at construction (bounded: one small leak per plugin load).
        Self {
            vm,
            id: Box::leak(id.to_string().into_boxed_str()),
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

#[async_trait]
impl SecretStore for JsSecretBackend {
    fn id(&self) -> &'static str {
        self.id
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
        let args = [json!({
            "masterPassword": creds.master_password.expose_secret(),
            "twoFactorToken": creds.two_factor_token.as_ref().map(|t| t.expose_secret()),
            "email": creds.email,
            "apiKeyClientId": creds.api_key_client_id.as_ref().map(|t| t.expose_secret()),
            "apiKeyClientSecret": creds.api_key_client_secret.as_ref().map(|t| t.expose_secret()),
        })];
        self.call_unit("secretUnlock", &args)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn vm_with(src: &str) -> Arc<PluginVm> {
        Arc::new(PluginVm::from_source("test-secret", src.to_string()).expect("vm"))
    }

    #[tokio::test]
    async fn status_and_get_roundtrip() {
        let vm = vm_with(
            r#"module.exports.default = {
                secretStatus() { return { status: "unlocked", user: "me@x.io", transient: false }; },
                secretGetItem(name) { return { ok: { item_id: "id1", name: name, value: "s3cr3t", notes: null } }; }
            }"#,
        );
        let b = JsSecretBackend::new(vm, "test-vault");
        assert_eq!(b.id(), "test-vault");
        assert!(b.status().await.is_unlocked());
        let v = b.get("api_key").await.unwrap();
        assert_eq!(v.name, "api_key");
        assert_eq!(v.value.expose_secret(), "s3cr3t");
    }

    #[tokio::test]
    async fn error_envelope_maps_to_secret_error() {
        let vm = vm_with(
            r#"module.exports.default = {
                secretGetItem() { return { error: { kind: "not_found" } }; }
            }"#,
        );
        let b = JsSecretBackend::new(vm, "test-vault");
        assert!(matches!(b.get("missing").await, Err(SecretError::NotFound)));
    }

    #[tokio::test]
    async fn optional_methods_default_when_absent() {
        let vm = vm_with(
            r#"module.exports.default = { secretStatus() { return { status: "locked" }; } }"#,
        );
        let b = JsSecretBackend::new(vm, "test-vault");
        assert!(b.organizations().await.unwrap().is_empty());
        assert!(b.sync().await.is_ok());
        assert!(b.init().await.is_ok());
    }

    // NOTE: Bitwarden-specific behaviour (bw CLI envelopes, cipher shape, error
    // mapping) is tested on the PLUGIN side — see plugins/bitwarden/plugin.test.cjs.
    // Core only tests the JsSecretBackend seam, with inline fixtures, above.
}
