//! One manifest seed, two runtimes: the bundled JS plugin and the native
//! provider must expose the identical Claude alias catalog, at seed and after
//! a genuine live contribution. Hermetic: no user file reads/writes, no
//! process-global `claude` overlay.

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

#[test]
fn claude_manifest_catalog_is_identical_for_native_and_js_without_credentials() {
    if require_plugins().is_none() {
        return;
    }
    let (manifest_models, manifest_default) = manifest_expectation();

    // Registration-level parity: both runtimes seed from the ONE bundled
    // manifest, so a drifted mirrored vector fails against the file itself.
    let native_seed = crate::agent_claude::provider::manifest_defaults();
    assert_eq!(
        native_seed.models, manifest_models,
        "native seed must be the bundled manifest, not a duplicated literal vector"
    );
    assert_eq!(native_seed.default, manifest_default);
    assert!(
        native_seed.models.iter().all(|m| m.available.is_none()),
        "static seed availability is unknown for every alias"
    );

    let (native, js) = pair();
    let js_seed: Vec<ModelInfo> = js.manifest().models.iter().map(ModelInfo::from).collect();
    assert_eq!(
        js_seed, manifest_models,
        "JS seed must be the bundled manifest, not a duplicated literal vector"
    );
    assert_eq!(js.manifest().default_model, manifest_default);

    // Resolved-path parity through the real providers. User overrides and live
    // state apply to both readers identically, so equality is unconditional —
    // this never asserts on (or mutates) the user's models directory.
    assert_eq!(
        native.available_models(),
        js.available_models(),
        "native and JS must agree field-for-field on the resolved catalog"
    );
    assert_eq!(native.default_model(), js.default_model());

    // Genuine live contribution, isolated under a fixture id: the real
    // plugin.js and the native model_catalog seam must see one identical
    // result, without overlaying the process-global `claude` entry.
    let _cleanup = FixtureCacheCleanup;
    let fixture_js = claude_js_under_fixture_id();
    let fixture_native = FixtureNativeReader;
    assert_eq!(fixture_js.available_models(), manifest_models);
    assert_eq!(fixture_native.available_models(), manifest_models);

    let live: Vec<ModelInfo> = manifest_models
        .iter()
        .map(|m| ModelInfo {
            available: Some(true),
            ..m.clone()
        })
        .collect();
    crate::model_catalog::put_live(LIVE_FIXTURE_ID, live.clone(), manifest_default.clone());

    assert_eq!(fixture_native.available_models(), live);
    assert_eq!(fixture_js.available_models(), live);
    assert_eq!(fixture_native.default_model(), manifest_default);
    assert_eq!(fixture_js.default_model(), manifest_default);
}

const LIVE_FIXTURE_ID: &str = "claude-catalog-live-parity-fixture";

/// Removes the fixture's cache artifact even when the test panics.
struct FixtureCacheCleanup;

impl Drop for FixtureCacheCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(
            crate::model_catalog::models_dir().join(format!("{LIVE_FIXTURE_ID}.cache.json")),
        );
    }
}

/// The real Claude plugin.js under a fixture manifest id, so the live leg
/// never overlays the process-global `claude` catalog entry.
fn claude_js_under_fixture_id() -> JsProvider {
    let plugins = workspace_plugins_root().join("provider-claude");
    let plugin_js = std::fs::read_to_string(plugins.join("plugin.js")).expect("read plugin.js");
    let mut manifest_json: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(plugins.join("plugin.json")).unwrap())
            .unwrap();
    manifest_json["id"] = serde_json::json!(LIVE_FIXTURE_ID);
    let manifest_json = manifest_json.to_string();
    let manifest: crate::js_runtime::PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = crate::js_runtime::vm::PluginVm::from_source_with(
        LIVE_FIXTURE_ID,
        plugin_js,
        Some(manifest_json),
    )
    .unwrap();
    JsProvider::new(manifest, std::sync::Arc::new(vm))
}

/// Native-path reader for the fixture id: the same `model_catalog` seam the
/// native ClaudeProvider's `available_models`/`default_model` delegate to.
struct FixtureNativeReader;

#[async_trait::async_trait]
impl AgentProvider for FixtureNativeReader {
    fn id(&self) -> &str {
        LIVE_FIXTURE_ID
    }
    fn display_name(&self) -> &str {
        "Claude catalog live parity fixture"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::provider::ReadinessStrategy {
        crate::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
        }
    }
    fn task_delivery(&self) -> crate::provider::TaskDelivery {
        crate::provider::TaskDelivery::PtyInput
    }
    fn detect_from_title(&self, _: &str) -> bool {
        false
    }
    fn detect_from_output(&self, _: &str) -> bool {
        false
    }
    fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
        None
    }
    fn detect_session_id(
        &self,
        _: &str,
        _: &[String],
        _: Option<std::time::Duration>,
    ) -> Option<String> {
        None
    }
    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }
    fn available_models(&self) -> Vec<ModelInfo> {
        crate::model_catalog::resolve_models(LIVE_FIXTURE_ID)
    }
    fn default_model(&self) -> Option<String> {
        crate::model_catalog::resolve_default(LIVE_FIXTURE_ID)
    }
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
