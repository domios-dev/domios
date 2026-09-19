//! Parity for the provider-declared model groups, per-group usage, and
//! credential files: the native and JS runtimes must agree, because the picker
//! and the catalog read them identically regardless of runtime.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use crate::provider::{
    AgentLaunchParams, AgentProvider, ModelGroup, ModelGroupUsage, ReadinessStrategy, TaskDelivery,
};

use super::JsProvider;

/// A provider that declares model groups, per-group usage, and a credential
/// file. The native stub and the JS fixture must agree on all three.
struct ModelGroupNativeStub;

#[async_trait::async_trait]
impl AgentProvider for ModelGroupNativeStub {
    fn id(&self) -> &str {
        "model-group-fixture"
    }

    fn display_name(&self) -> &str {
        "Model Group Fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::ZERO,
        }
    }

    fn task_delivery(&self) -> TaskDelivery {
        TaskDelivery::PtyInput
    }

    fn detect_from_title(&self, _: &str) -> bool {
        false
    }

    fn detect_from_output(&self, _: &str) -> bool {
        false
    }

    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
        None
    }

    fn detect_session_id(&self, _: &str, _: &[String], _: Option<Duration>) -> Option<String> {
        None
    }

    fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
        None
    }

    fn model_groups(&self) -> Vec<ModelGroup> {
        vec![
            ModelGroup {
                id: "alpha".into(),
                label: "Alpha Co".into(),
            },
            ModelGroup {
                id: "beta".into(),
                label: "Beta Co".into(),
            },
        ]
    }

    fn model_group_usage(&self) -> Vec<ModelGroupUsage> {
        vec![ModelGroupUsage {
            group: "alpha".into(),
            messages: 3,
            input_tokens: 120,
            output_tokens: 40,
            cache_read_tokens: 10,
            cache_write_tokens: 5,
            cost_micros: 1_500_000,
            spend_cents: Some(150.0),
            spend_limit_cents: None,
            spend_remaining_cents: Some(62.0),
            spend_resets_at_ms: None,
        }]
    }

    fn reports_group_usage(&self) -> bool {
        true
    }

    fn model_credential_files(&self) -> Vec<PathBuf> {
        vec![PathBuf::from("/tmp/model-group-fixture-auth.json")]
    }
}

#[test]
fn model_groups_usage_and_credentials_match_native_and_js() {
    let manifest_json = r#"{
        "id":"model-group-fixture",
        "version":"0.1.0",
        "displayName":"Model Group Fixture",
        "spawn":{"readinessStrategy":{"kind":"fixed_delay","durationMs":0},
                 "taskDelivery":"pty_input","verifySubmit":{"kind":"none"}},
        "modelGroups":[{"id":"alpha","label":"Alpha Co"},{"id":"beta","label":"Beta Co"}],
        "credentials":[{"id":"fixtureAuth","file":"/tmp/model-group-fixture-auth.json"}]
    }"#;
    let plugin_js = "module.exports.default = { modelGroupUsage() { return [\
        {group:'alpha',messages:3,input_tokens:120,output_tokens:40,\
         cache_read_tokens:10,cache_write_tokens:5,cost_micros:1500000,\
         spend_cents:150,spend_remaining_cents:62}]; } };";
    let manifest: super::super::PluginManifest = serde_json::from_str(manifest_json).unwrap();
    let vm = super::super::PluginVm::from_source_with(
        "model-group-fixture",
        plugin_js.to_string(),
        Some(manifest_json.to_string()),
    )
    .expect("fixture vm");
    let js = JsProvider::new(manifest, Arc::new(vm));
    let native = ModelGroupNativeStub;

    assert_eq!(js.model_groups(), native.model_groups());
    assert_eq!(js.model_group_usage(), native.model_group_usage());
    assert_eq!(js.model_credential_files(), native.model_credential_files());
    assert_eq!(js.reports_group_usage(), native.reports_group_usage());
    assert_eq!(js.describe().model_groups, native.describe().model_groups);
    // A provider that publishes per-group usage still earns the Usage Tracking
    // toggle, so the group-spend capsule is gated like any other usage surface.
    assert_eq!(
        js.describe().settings_schema,
        native.describe().settings_schema,
        "group-usage providers must expose identical usage-tracking settings"
    );
    assert!(
        !native.describe().settings_schema.is_empty(),
        "the native group-usage stub must contribute a settings section"
    );
}
