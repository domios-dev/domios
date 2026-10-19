use super::super::{PluginManifest, PluginVm};
use super::*;

struct RequiredLiveNativeStub {
    id: String,
    display_name: String,
    model_catalogue: crate::provider::ModelCatalogueCapability,
}

#[async_trait::async_trait]
impl AgentProvider for RequiredLiveNativeStub {
    fn id(&self) -> &str {
        &self.id
    }

    fn display_name(&self) -> &str {
        &self.display_name
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
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

    fn model_catalogue_capability(&self) -> crate::provider::ModelCatalogueCapability {
        self.model_catalogue.clone()
    }
}

fn provider_with_config_environments() -> (PluginManifest, JsProvider) {
    let root = super::workspace_plugins_root();
    let manifest_path = std::fs::read_dir(root)
        .expect("bundled plugins directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path().join("plugin.json"))
        .find(|path| {
            PluginManifest::load(path)
                .is_ok_and(|manifest| !manifest.model_catalogue.config_file_environments.is_empty())
        })
        .expect("a bundled provider declares config environment roots");
    let manifest = PluginManifest::load(&manifest_path).unwrap();
    let provider = load_one(&manifest_path).unwrap();
    (manifest, provider)
}

#[test]
fn model_catalogue_authority_native_js_parity() {
    use std::sync::Arc;

    let (manifest, js_required_live) = provider_with_config_environments();
    let native_required_live = RequiredLiveNativeStub {
        id: manifest.id.clone(),
        display_name: manifest.display_name.clone(),
        model_catalogue: manifest.model_catalogue,
    };
    assert_eq!(
        native_required_live.model_catalogue_capability(),
        js_required_live.model_catalogue_capability()
    );

    let manifest_json = r#"{
        "id":"discover-fixture",
        "version":"0.1.0",
        "displayName":"Discover Fixture",
        "modelCatalogue":{"authority":"authoritative","liveDiscoveryRequired":true},
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                 "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}}
    }"#;
    let manifest: PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = PluginVm::from_source_with(
        "discover-fixture",
        "module.exports.default = {};".into(),
        Some(manifest_json.into()),
    )
    .unwrap();
    let js_discover = JsProvider::new(manifest, Arc::new(vm));
    let native_discover = DiscoverModelsNativeStub;
    assert_eq!(
        native_discover.model_catalogue_capability(),
        js_discover.model_catalogue_capability()
    );
    assert_eq!(
        native_discover.describe().model_catalogue,
        js_discover.describe().model_catalogue
    );
}
