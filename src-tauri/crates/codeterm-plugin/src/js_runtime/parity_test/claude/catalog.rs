//! One manifest seed, two runtimes: the bundled JS plugin and the native
//! provider must expose the identical Claude alias catalog without
//! credentials, and the identical result after a genuine live contribution.

use super::*;

fn manifest_expectation() -> (Vec<ModelInfo>, Option<String>) {
    let manifest_path = workspace_plugins_root()
        .join("provider-claude")
        .join("plugin.json");
    let manifest: crate::js_runtime::PluginManifest =
        serde_json::from_str(&std::fs::read_to_string(&manifest_path).expect("read manifest"))
            .expect("bundled Claude manifest parses");
    (
        manifest.models.iter().map(ModelInfo::from).collect(),
        manifest.default_model,
    )
}

fn claude_cache_file() -> std::path::PathBuf {
    crate::model_catalog::models_dir().join("claude.cache.json")
}

#[test]
fn claude_manifest_catalog_is_identical_for_native_and_js_without_credentials() {
    if require_plugins().is_none() {
        return;
    }
    let (manifest_models, manifest_default) = manifest_expectation();
    assert!(
        !crate::model_catalog::models_dir().join("claude.toml").exists(),
        "user override present — this oracle needs the pristine registered seed"
    );
    let _ = std::fs::remove_file(claude_cache_file());

    // Real provider paths: native registers via include_str! of the manifest,
    // the JS plugin registers via its parsed manifest — one seed, two readers.
    let (native, js) = pair();

    let native_models = native.available_models();
    let js_models = js.available_models();
    assert_eq!(
        native_models, js_models,
        "native and JS must agree field-for-field on the seed catalog"
    );
    assert_eq!(
        native_models, manifest_models,
        "the resolved catalog must be the bundled manifest seed, not a mirrored vector"
    );
    assert_eq!(native.default_model(), manifest_default);
    assert_eq!(js.default_model(), manifest_default);
    assert!(
        native_models.iter().all(|m| m.available.is_none()),
        "static seed availability is unknown for every alias"
    );

    // Genuine live contribution: same stable alias ids, account availability.
    let live: Vec<ModelInfo> = manifest_models
        .iter()
        .map(|m| ModelInfo {
            available: Some(true),
            ..m.clone()
        })
        .collect();
    crate::model_catalog::put_live("claude", live.clone(), manifest_default.clone());
    assert_eq!(native.available_models(), live);
    assert_eq!(js.available_models(), live);

    // Restore the registered seed and drop the test cache artifact.
    crate::model_catalog::register(
        "claude",
        crate::model_catalog::RegisteredDefaults {
            models: vec![],
            default: None,
        },
    );
    crate::model_catalog::register(
        "claude",
        crate::model_catalog::RegisteredDefaults {
            models: manifest_models,
            default: manifest_default,
        },
    );
    let _ = std::fs::remove_file(claude_cache_file());
}

#[test]
fn claude_manifest_aliases_map_to_concrete_execution_ids() {
    if require_plugins().is_none() {
        return;
    }
    let (manifest_models, manifest_default) = manifest_expectation();
    let ids: Vec<&str> = manifest_models.iter().map(|m| m.id.as_str()).collect();
    assert_eq!(ids, ["opus", "sonnet", "haiku", "fable"]);
    assert_eq!(manifest_default.as_deref(), Some("sonnet"));
    for alias in ids {
        let concrete = codeterm_types::agent::models::resolve_model_id(alias);
        assert_ne!(concrete, alias, "alias {alias} must map to a concrete id");
        assert_eq!(
            codeterm_types::agent::models::canonicalize_short(concrete),
            alias,
            "concrete id {concrete} must canonicalize back to {alias}"
        );
    }
}
