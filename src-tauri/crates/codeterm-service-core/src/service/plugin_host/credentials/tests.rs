use super::*;

async fn cleanup(
    store: &dyn SecretStore,
    bucket: SecretFile,
    context: &InvocationContext,
    declaration: Option<&AgentRuntimeCapability>,
) -> Result<(), String> {
    super::cleanup_with(
        store,
        bucket,
        context,
        declaration,
        tokio::time::Instant::now() + Duration::from_secs(10),
    )
    .await
}
use crate::secrets::{mock::MockStore, SaveRequest, SecretString};
use codeterm_plugin::provider_account::InvocationContext;
use serde_json::json;
use std::{collections::BTreeMap, path::Path, sync::Arc};

fn context(account: &str, home: &Path) -> Arc<InvocationContext> {
    let capability = serde_json::from_value(json!({"homeEnvironment":"FIXTURE_HOME","defaultHome":"~/.fixture","secretEnvironment":[],"isolatedPaths":[],"sharedPaths":[],"continuation":"unverified"})).unwrap();
    InvocationContext::from_capability(
        "fixture".into(),
        account.into(),
        &capability,
        BTreeMap::from([("FIXTURE_HOME".into(), Some(home.to_str().unwrap().into()))]),
        vec![home.into()],
        vec![],
    )
    .unwrap()
}

fn declaration() -> AgentRuntimeCapability {
    serde_json::from_value(json!({
        "runtime":"stdio-jsonrpc","command":{"binary":"fixture","args":[]},"requestMethods":{},"notificationMethods":[],
        "launchProfiles":{"profile":{"binary":"fixture","args":[],
            "authEnv":{"KEY":{"secret":"domios:provider:fixture:api-key","methods":["key"]}},
            "secretEnv":{"INSTALL":"domios:provider:fixture:install-key","EXTERNAL":"owner:shared-key"}
        }}
    })).unwrap()
}

async fn save(store: &dyn SecretStore, name: &str) {
    store
        .save(SaveRequest {
            name: name.into(),
            value: SecretString::new("opaque-fixture-value".into()),
            notes: None,
            folder: None,
            organization: None,
            collection: None,
            overwrite: true,
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn account_home_cleanup_deletes_only_one_roots_bucket_and_declared_host_keys() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("staged");
    let other = root.path().join("other");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    let staged = context("auth-stage", &home);
    let promoted = context("a", &home);
    let sibling = context("b", &other);
    let store = MockStore::new();
    let declaration = declaration();
    let declared = [
        "domios:provider:fixture:api-key",
        "domios:provider:fixture:install-key",
    ];
    let scoped: Vec<_> = declared
        .iter()
        .map(|name| secret_reference("fixture", name, Some(&staged)).unwrap())
        .collect();
    let sibling_keys: Vec<_> = declared
        .iter()
        .map(|name| secret_reference("fixture", name, Some(&sibling)).unwrap())
        .collect();
    let undeclared = secret_reference(
        "fixture",
        "domios:provider:fixture:undeclared",
        Some(&staged),
    )
    .unwrap();
    for key in scoped
        .iter()
        .chain(&sibling_keys)
        .map(String::as_str)
        .chain(declared)
        .chain([undeclared.as_str(), "owner:shared-key"])
    {
        save(&store, key).await;
    }
    let promoted_key = secret_reference("fixture", declared[0], Some(&promoted)).unwrap();
    assert_eq!(promoted_key, scoped[0]);
    assert!(store.get(&promoted_key).await.is_ok());
    let bucket =
        codeterm_util::secret_file::plugin_secret_bucket(root.path().join("vault"), "fixture");
    let foreign =
        codeterm_util::secret_file::plugin_secret_bucket(root.path().join("vault"), "foreign");
    let prefix = staged.credential_secret_prefix("fixture").unwrap();
    let sibling_prefix = sibling.credential_secret_prefix("fixture").unwrap();
    for name in [
        format!("{prefix}token"),
        format!("{prefix}another"),
        format!("{sibling_prefix}token"),
        "token".into(),
    ] {
        bucket.set(&name, "opaque-fixture-value").unwrap();
    }
    foreign
        .set(&format!("{prefix}token"), "opaque-foreign")
        .unwrap();
    cleanup(&store, bucket.clone(), &promoted, Some(&declaration))
        .await
        .unwrap();
    for key in &scoped {
        assert!(matches!(store.get(key).await, Err(SecretError::NotFound)));
    }
    for key in sibling_keys
        .iter()
        .map(String::as_str)
        .chain(declared)
        .chain([undeclared.as_str(), "owner:shared-key"])
    {
        assert!(store.get(key).await.is_ok());
    }
    assert!(bucket.get(&format!("{prefix}token")).is_err());
    assert!(bucket.get(&format!("{prefix}another")).is_err());
    assert!(bucket.get(&format!("{sibling_prefix}token")).is_ok());
    assert!(bucket.get("token").is_ok());
    assert!(foreign.get(&format!("{prefix}token")).is_ok());
    cleanup(&store, bucket, &promoted, Some(&declaration))
        .await
        .unwrap();
}

#[tokio::test]
async fn account_home_cli_cleanup_needs_no_runtime_or_available_external_store() {
    let root = tempfile::tempdir().unwrap();
    let invocation = context("auth-stage", root.path());
    let bucket =
        codeterm_util::secret_file::plugin_secret_bucket(root.path().join("vault"), "fixture");
    let key = format!(
        "{}token",
        invocation.credential_secret_prefix("fixture").unwrap()
    );
    bucket.set(&key, "opaque-fixture-value").unwrap();
    bucket.set("token", "default-fixture-value").unwrap();
    let store = MockStore::new_unavailable("fixture unavailable");
    assert!(
        cleanup(&store, bucket.clone(), &invocation, Some(&declaration()))
            .await
            .is_err()
    );
    assert!(bucket.has(&key).unwrap());
    cleanup(&store, bucket.clone(), &invocation, None)
        .await
        .unwrap();
    assert!(!bucket.has(&key).unwrap());
    assert!(bucket.has("token").unwrap());
    let raw = InvocationContext::new(
        "fixture".into(),
        "a".into(),
        BTreeMap::new(),
        vec![],
        vec![],
    )
    .unwrap();
    assert!(cleanup(&store, bucket.clone(), &raw, None).await.is_err());
    assert!(bucket.has("token").unwrap());
    assert!(cleanup(
        &store,
        SecretFile::in_memory("unavailable persistence"),
        &invocation,
        None
    )
    .await
    .is_err());
}
