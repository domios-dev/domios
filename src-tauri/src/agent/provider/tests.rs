use super::*;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn usage_tracking_section_defaults_on() {
    let section = usage_tracking_section("/usage");
    match section {
        SettingField::Section { fields, title, .. } => {
            assert_eq!(title, "Usage Tracking");
            assert_eq!(fields.len(), 1);
            match &fields[0] {
                SettingField::Toggle { key, default, .. } => {
                    assert_eq!(key, "usage_tracking_enabled");
                    assert!(*default, "tracking must default to ON");
                }
                other => panic!("expected Toggle, got {:?}", other),
            }
        }
        other => panic!("expected Section, got {:?}", other),
    }
}

/// Round-trip golden test for the wire format.
/// Catches accidental field renames or breaking serde changes
/// before they reach external consumers.
#[test]
fn provider_descriptor_round_trip() {
    let original = ProviderDescriptor {
            schema_version: PROVIDER_DESCRIPTOR_SCHEMA_VERSION,
            id: "test".into(),
            display_name: "Test Provider".into(),
            icon_hint: Some("T".into()),
            icon_svg: Some("<svg viewBox=\"0 0 24 24\" fill=\"currentColor\"><circle cx=\"12\" cy=\"12\" r=\"6\"/></svg>".into()),
            icon_color: Some("#abcdef".into()),
            available_models: vec![ModelInfo {
                id: "model-1".into(),
                display_name: "Model One".into(),
                description: Some("first model".into()),
                available: None,
            }],
            default_model: Some("model-1".into()),
            reasoning_efforts: vec![ReasoningEffortInfo {
                id: "high".into(),
                display_name: "High".into(),
                description: Some("deeper reasoning".into()),
            }],
            default_reasoning_effort: Some("high".into()),
            settings_schema: vec![
                SettingField::Section {
                    title: "Group".into(),
                    description: Some("desc".into()),
                    fields: vec![
                        SettingField::Toggle {
                            key: "flag".into(),
                            label: "Flag".into(),
                            default: true,
                            description: None,
                        },
                        SettingField::When {
                            field: "flag".into(),
                            equals: serde_json::json!(true),
                            then: vec![SettingField::Info {
                                title: None,
                                body: "Visible when flag is on".into(),
                                level: NoteLevel::Warn,
                            }],
                        },
                    ],
                },
                SettingField::Action {
                    id: "do_thing".into(),
                    label: "Do Thing".into(),
                    command: "test_command".into(),
                    state_query: Some("query_state".into()),
                    args: None,
                    confirm: None,
                    style: ActionStyle::Primary,
                },
                SettingField::Badge {
                    query_command: "check_state".into(),
                    query_args: None,
                    variants: vec![BadgeVariant {
                        id: "ok".into(),
                        label: "OK".into(),
                        style: BadgeStyle::Green,
                    }],
                },
            ],
            notifications_schema: vec![SettingField::Section {
                title: "Notify".into(),
                description: None,
                fields: vec![SettingField::Toggle {
                    key: "notify_on_idle".into(),
                    label: "Notify on idle".into(),
                    default: true,
                    description: None,
                }],
            }],
            supports_lazy_skills: true,
            has_hooks: true,
            supports_permission_bypass: true,
            cli_binary: Some("testcli".into()),
            install_command: Some("npm i -g testcli".into()),
            model_switch_format: Some("/model {model}".into()),
            fresh_context_command: Some("/new".into()),
            compact_command: Some("/compact".into()),
            self_update: Some(ProviderSelfUpdateCapability {
                in_session: true,
                restart_required: true,
                completion_signal: Some(ProviderUpdateCompletionSignal::ProviderClassification),
            }),
            supports_stop: true,
            supports_chat_ui: true,
            supports_structured_prompts: true,
            supports_watcher: false,
            trust: ProviderTrust::System,
            transport: ProviderTransport::ChatBackend,
            behavior_config: BehaviorConfig::PresetsAndSystemPrompt,
            presets: vec![crate::chatbackend::PresetInfo {
                id: "p1".into(),
                name: "Preset One".into(),
                description: Some("a preset".into()),
            }],
            system_prompt_delivery: SystemPromptDelivery::CliFlag("--system".into()),
        };

    let json = serde_json::to_string(&original).expect("serialize");
    let decoded: ProviderDescriptor = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(original, decoded, "round-trip must be lossless");
}

struct NativeProviderUpdateFixture;

#[async_trait::async_trait]
impl AgentProvider for NativeProviderUpdateFixture {
    fn id(&self) -> &str {
        "provider-update-fixture"
    }

    fn display_name(&self) -> &str {
        "Provider Update Fixture"
    }

    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }

    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::ZERO,
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

    fn provider_self_update_capability(&self) -> Option<ProviderSelfUpdateCapability> {
        Some(ProviderSelfUpdateCapability {
            in_session: true,
            restart_required: true,
            completion_signal: Some(ProviderUpdateCompletionSignal::ProviderClassification),
        })
    }
}

#[test]
fn provider_update_policy_has_native_js_capability_parity() {
    use std::sync::Arc;

    let manifest_json = r#"{
        "id":"provider-update-fixture",
        "version":"0.1.0",
        "displayName":"Provider Update Fixture",
        "spawn":{
            "readinessStrategy":{"kind":"fixed_delay","durationMs":0},
            "taskDelivery":"pty_input",
            "verifySubmit":{"kind":"none"}
        }
    }"#;
    let manifest: codeterm_plugin::js_runtime::PluginManifest =
        serde_json::from_str(manifest_json).expect("manifest parses");
    let vm = codeterm_plugin::js_runtime::PluginVm::from_source_with(
        "provider-update-fixture",
        "module.exports.default = { providerSelfUpdateCapability() { return { inSession: true, restartRequired: true, completionSignal: 'provider_classification' }; } };".to_string(),
        Some(manifest_json.to_string()),
    )
    .expect("fixture vm");
    let js = codeterm_plugin::js_runtime::JsProvider::new(manifest, Arc::new(vm));
    let native = NativeProviderUpdateFixture;

    assert_eq!(
        js.provider_self_update_capability(),
        native.provider_self_update_capability()
    );
    assert_eq!(js.describe().self_update, native.describe().self_update);
}

#[test]
fn provider_descriptor_rejects_unknown_fields() {
    // schema_version protects against silent breakage; deny_unknown_fields protects against typos in field names.
    let bad = r#"{
            "schema_version": 1,
            "id": "test",
            "display_name": "Test",
            "available_models": [],
            "settings_schema": [],
            "notifications_schema": [],
            "supports_lazy_skills": false,
            "has_hooks": false,
            "model_switch_format": "/model {model}",
            "extra_unknown_field": "boom"
        }"#;
    let result: Result<ProviderDescriptor, _> = serde_json::from_str(bad);
    assert!(result.is_err(), "deny_unknown_fields must reject extras");
}

/// Minimal stub provider — exercises `describe()` default impl through
/// the trait, ensuring it compiles and assembles correctly.
struct StubProvider;

impl AgentProvider for StubProvider {
    fn id(&self) -> &str {
        "stub"
    }
    fn display_name(&self) -> &str {
        "Stub"
    }
    fn build_launch_command(&self, _: &AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> ReadinessStrategy {
        ReadinessStrategy::FixedDelay {
            duration: Duration::from_secs(0),
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
}

#[test]
fn default_describe_assembles_from_methods() {
    let p = StubProvider;
    let desc = p.describe();
    assert_eq!(desc.schema_version, PROVIDER_DESCRIPTOR_SCHEMA_VERSION);
    assert_eq!(desc.id, "stub");
    assert_eq!(desc.display_name, "Stub");
    assert!(desc.available_models.is_empty());
    assert!(desc.settings_schema.is_empty());
    assert!(!desc.supports_lazy_skills); // default false
    assert!(!desc.has_hooks); // default false
    assert!(!desc.supports_structured_prompts); // default false
    assert_eq!(desc.transport, ProviderTransport::Pty);
    assert_eq!(desc.behavior_config, BehaviorConfig::OwnPromptSet);
    assert!(desc.presets.is_empty());
    assert_eq!(desc.system_prompt_delivery, SystemPromptDelivery::External);
}

#[test]
fn serialized_descriptor_has_transport_not_kind() {
    let value = serde_json::to_value(StubProvider.describe()).unwrap();
    assert!(value.get("kind").is_none(), "`kind` must not be serialized");
    assert_eq!(value["transport"], "pty");
}
