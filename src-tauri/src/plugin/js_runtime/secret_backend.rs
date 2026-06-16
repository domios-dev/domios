//! `JsSecretBackend` — adapts a JS plugin that declares `capabilities.secret_backend`
//! to the core [`SecretStore`](crate::secrets::SecretStore) trait. Mirrors
//! `JsProvider`/`JsTranscriber`: wraps the shared `Arc<PluginVm>` and dispatches to
//! exported plugin functions via the synchronous `call_json` on the caller's thread
//! (the VM is not `Send`).
//!
//! Wire contract: each method returns an envelope `{ ok: <data> }` or
//! `{ error: { kind, ... } }` (the error shape is `SecretError`). `secretStatus`
//! returns a bare `StoreStatus`. Optional methods (organizations/collections/sync/
//! init) fall back to the trait default when the plugin doesn't export them.

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
        self.call::<Vec<SecretRef>>("secretList", &[serde_json::to_value(&filter).unwrap_or(json!({}))])
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
        let b = JsSecretBackend::new(vm, "bitwarden");
        assert_eq!(b.id(), "bitwarden");
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
        let b = JsSecretBackend::new(vm, "bitwarden");
        assert!(matches!(b.get("missing").await, Err(SecretError::NotFound)));
    }

    #[tokio::test]
    async fn optional_methods_default_when_absent() {
        let vm = vm_with(r#"module.exports.default = { secretStatus() { return { status: "locked" }; } }"#);
        let b = JsSecretBackend::new(vm, "bitwarden");
        assert!(b.organizations().await.unwrap().is_empty());
        assert!(b.sync().await.is_ok());
        assert!(b.init().await.is_ok());
    }

    // --- Real shipped Bitwarden plugin: loads + pure-logic parity with native Rust ---

    fn bitwarden_manifest() -> std::path::PathBuf {
        std::path::PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
            .join("plugins/bitwarden/plugin.json")
    }

    #[test]
    fn shipped_bitwarden_loads_with_secret_capability() {
        let mp = bitwarden_manifest();
        if !mp.exists() {
            return;
        }
        let p = super::super::loader::load_one(&mp).expect("load bitwarden");
        assert_eq!(p.manifest().id, "bitwarden");
        assert!(p.manifest().capabilities.secret_backend);
        assert!(!p.manifest().capabilities.agent_provider);
        assert_eq!(p.manifest().host_api, Some(2));
        // subprocess allowlist carries bw.
        assert!(p
            .manifest()
            .permissions
            .subprocess
            .allow
            .iter()
            .any(|b| b == "bw"));
    }

    #[test]
    fn shipped_bitwarden_pure_helpers_match_native() {
        use serde_json::json;
        let mp = bitwarden_manifest();
        if !mp.exists() {
            return;
        }
        let p = super::super::loader::load_one(&mp).expect("load bitwarden");
        let vm = p.vm();

        // base64 — standard encoding, parity with the Rust STANDARD engine.
        let b64: String = call_json(vm, "__test_base64", &[json!("hello")]).unwrap();
        assert_eq!(b64, "aGVsbG8=");

        // error mapping — mirror of map_bw_message().
        let locked: serde_json::Value =
            call_json(vm, "__test_mapError", &[json!("Vault is locked.")]).unwrap();
        assert_eq!(locked["kind"], "locked");
        let nf: serde_json::Value =
            call_json(vm, "__test_mapError", &[json!("Item Not found.")]).unwrap();
        assert_eq!(nf["kind"], "not_found");
        let logged_out: serde_json::Value =
            call_json(vm, "__test_mapError", &[json!("You are not logged in.")]).unwrap();
        assert_eq!(logged_out["kind"], "logged_out");
        let exists: serde_json::Value =
            call_json(vm, "__test_mapError", &[json!("An item with that name already exists.")])
                .unwrap();
        assert_eq!(exists["kind"], "already_exists");

        // envelope unwrap — mirror of unwrap_data_envelope().
        let unwrapped: serde_json::Value =
            call_json(vm, "__test_unwrap", &[json!({ "object": "list", "data": [1, 2] })]).unwrap();
        assert_eq!(unwrapped, json!([1, 2]));

        // cipher build — mirror of build_login_cipher() (type=1 login, password in login).
        let cipher: serde_json::Value = call_json(
            vm,
            "__test_buildCipher",
            &[json!({ "name": "n", "value": "p", "notes": null }), json!(null), json!(null)],
        )
        .unwrap();
        assert_eq!(cipher["type"], 1);
        assert_eq!(cipher["name"], "n");
        assert_eq!(cipher["login"]["password"], "p");
    }
}
