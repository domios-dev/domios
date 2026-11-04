mod output_detection;
use super::*;
use crate::plugin::agent_claude::ClaudeProvider;

fn registry_with_claude() -> AgentRegistry {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(ClaudeProvider::new()));
    reg
}

/// Minimal provider used to exercise the native-fallback bookkeeping.
/// `native` toggles `is_native()` so we can model both a real native impl
/// and a JS drop-in without spinning a VM.
struct FallbackStub {
    id: &'static str,
    native: bool,
}

impl crate::agent::provider::AgentProvider for FallbackStub {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.id
    }
    fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
        String::new()
    }
    fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
        crate::agent::provider::ReadinessStrategy::FixedDelay {
            duration: std::time::Duration::from_secs(0),
        }
    }
    fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
        crate::agent::provider::TaskDelivery::PtyInput
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
    fn available_models(&self) -> Vec<crate::agent::provider::ModelInfo> {
        crate::plugin::model_catalog::resolve_models(self.id)
    }
    fn is_native(&self) -> bool {
        self.native
    }
}

/// #16 regression: a JS-only provider (no native sibling) re-registered on
/// hot-reload must NOT be captured as its own native fallback. Otherwise
/// `JsProvider::native()` resolves to itself and capability delegation
/// recurses until the stack overflows.
#[test]
fn js_only_provider_never_becomes_native_fallback() {
    let reg = AgentRegistry::new();
    // (hot-reload seeding tick) — the second displaces the first.
    reg.register_provider(Box::new(FallbackStub {
        id: "drop-in",
        native: false,
    }));
    reg.register_provider(Box::new(FallbackStub {
        id: "drop-in",
        native: false,
    }));
    assert!(
        reg.native_fallback("drop-in").is_none(),
        "a JS drop-in displaced by re-registration must never be a fallback"
    );
}

/// A JS provider displacing a genuine native impl still records the native
/// one as the fallback (the legitimate delegation case), and re-registration
/// keeps it native.
#[test]
fn native_impl_is_preserved_as_fallback_across_reregistration() {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(FallbackStub {
        id: "dual",
        native: true,
    }));
    reg.register_provider(Box::new(FallbackStub {
        id: "dual",
        native: false,
    }));
    let fb = reg
        .native_fallback("dual")
        .expect("native fallback recorded");
    assert!(fb.is_native(), "fallback must be the native impl");
    // Hot-reload re-registers the JS provider again — fallback stays native.
    reg.register_provider(Box::new(FallbackStub {
        id: "dual",
        native: false,
    }));
    assert!(reg.native_fallback("dual").unwrap().is_native());
}

#[test]
fn test_empty_registry() {
    let reg = AgentRegistry::new();
    assert!(reg.is_empty());
    assert!(reg.detect_from_title("Claude Code session").is_none());
}

#[test]
fn test_register_and_get() {
    let reg = registry_with_claude();
    assert!(!reg.is_empty());
    assert!(reg
        .with_provider("claude", |p| p.id().to_string())
        .is_some());
}

#[test]
fn test_unregister() {
    let reg = registry_with_claude();
    assert!(reg.unregister_provider("claude"));
    assert!(reg.is_empty());
    assert!(!reg.unregister_provider("claude"));
}

#[test]
fn test_registry_get_unknown() {
    let reg = AgentRegistry::new();
    assert!(reg.with_provider("nonexistent", |_| ()).is_none());
}

#[test]
fn startup_wait_unblocks_when_provider_registers() {
    let reg = Arc::new(AgentRegistry::new());
    reg.begin_startup_provider_loading(["slow"]);

    let worker_reg = reg.clone();
    let worker = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(25));
        worker_reg.register_provider(Box::new(FallbackStub {
            id: "slow",
            native: false,
        }));
    });

    assert_eq!(
        reg.wait_for_provider_startup_with_timeout("slow", Duration::from_secs(1)),
        ProviderStartupStatus::Ready
    );
    worker
        .join()
        .expect("provider registration thread panicked");
}

#[test]
fn startup_wait_reports_loading_before_unknown() {
    let reg = AgentRegistry::new();
    reg.begin_startup_provider_loading(["slow"]);

    assert_eq!(
        reg.wait_for_provider_startup_with_timeout("slow", Duration::from_millis(1)),
        ProviderStartupStatus::LoadingTimedOut
    );
    reg.finish_startup_provider_loading();
    assert_eq!(
        reg.wait_for_provider_startup_with_timeout("slow", Duration::from_millis(1)),
        ProviderStartupStatus::Unknown
    );
}

#[test]
fn register_if_absent_clears_startup_loading_for_pinned_native() {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(FallbackStub {
        id: "pinned",
        native: true,
    }));
    reg.begin_startup_provider_loading(["pinned"]);

    assert!(!reg.register_provider_if_absent(Box::new(FallbackStub {
        id: "pinned",
        native: false,
    })));
    assert_eq!(
        reg.wait_for_provider_startup_with_timeout("pinned", Duration::from_millis(1)),
        ProviderStartupStatus::Ready
    );
}

#[test]
fn test_detect_from_title() {
    let reg = registry_with_claude();
    let id = reg.detect_from_title("Claude Code session");
    assert_eq!(id.as_deref(), Some("claude"));
    assert!(reg.detect_from_title("bash shell").is_none());
}

#[test]
fn test_detect_from_output() {
    let reg = registry_with_claude();
    let id = reg.detect_from_output("Do you want to proceed?");
    assert_eq!(id.as_deref(), Some("claude"));
    assert!(reg.detect_from_output("normal output").is_none());
}

#[test]
fn test_provider_ids() {
    let reg = registry_with_claude();
    assert_eq!(reg.provider_ids(), vec!["claude".to_string()]);
}

#[test]
fn list_descriptors_returns_one_per_provider() {
    let reg = registry_with_claude();
    let descs = reg.list_descriptors();
    assert_eq!(descs.len(), 1);
    assert_eq!(descs[0].id, "claude");
    assert_eq!(descs[0].display_name, "Claude Code");
    assert!(!descs[0].available_models.is_empty());
    assert!(!descs[0].settings_schema.is_empty());
}

#[test]
fn listed_models_share_the_live_cache_registry() {
    let id = "test-listed-models-share-live-cache";
    crate::plugin::model_catalog::register(
        id,
        crate::plugin::model_catalog::RegisteredDefaults {
            models: vec![crate::agent::provider::ModelInfo {
                default_reasoning_effort: None,
                id: "phantom-model".into(),
                display_name: "Phantom Model".into(),
                description: None,
                available: None,
                availability: None,
                primary: None,
                featured: None,
                badge: None,
                group: None,
                reasoning_efforts: None,
            }],
            default: Some("phantom-model".into()),
        },
    );
    crate::agent::model_catalog::put_live(
        id,
        vec![crate::agent::provider::ModelInfo {
            default_reasoning_effort: None,
            id: "cache-model".into(),
            display_name: "Cache Model".into(),
            description: None,
            available: None,
            availability: None,
            primary: None,
            featured: None,
            badge: None,
            group: None,
            reasoning_efforts: None,
        }],
        Some("cache-model".into()),
    );

    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(FallbackStub { id, native: false }));
    let cached: Vec<_> = crate::agent::model_catalog::resolve_models(id)
        .into_iter()
        .map(|model| model.id)
        .collect();
    let listed: Vec<_> = reg
        .list_descriptors()
        .remove(0)
        .available_models
        .into_iter()
        .map(|model| model.id)
        .collect();

    let _ = std::fs::remove_file(
        crate::agent::model_catalog::models_dir().join(format!("{id}.cache.json")),
    );
    assert_eq!(listed, cached, "agent models must list the cached snapshot");
}

#[test]
fn list_descriptors_with_two_providers() {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(ClaudeProvider::new()));
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "codex",
    )));
    let descs = reg.list_descriptors();
    assert_eq!(descs.len(), 2);
    let ids: std::collections::HashSet<&str> = descs.iter().map(|d| d.id.as_str()).collect();
    assert!(ids.contains("claude"));
    assert!(ids.contains("codex"));
}

#[test]
fn restore_dispatch_builds_resume_for_every_provider() {
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(ClaudeProvider::new()));
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "codex",
    )));
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "opencode",
    )));
    for (id, sid) in [
        ("claude", "f00fc0dd-be1c-4d43-bb1d-212c923b75c1"),
        ("codex", "019e15cd-1527-7e21-9477-2631a52e499a"),
        ("opencode", "ses_1ea31e6d2ffe8WrqZl6g4mc6l2"),
    ] {
        let cmd = reg
            .with_provider(id, |p| p.build_resume_command(sid, true))
            .flatten()
            .unwrap_or_else(|| panic!("provider {} returned no resume command", id));
        assert!(
            cmd.contains(sid),
            "provider {} resume command missing sid: {}",
            id,
            cmd
        );
    }
}

#[test]
fn restore_dispatch_falls_back_to_fresh_launch_when_no_resume() {
    use crate::agent::provider::AgentLaunchParams;
    let reg = AgentRegistry::new();
    reg.register_provider(Box::new(ClaudeProvider::new()));
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "codex",
    )));
    reg.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "opencode",
    )));
    for id in ["claude", "codex", "opencode"] {
        let cmd = reg.with_provider(id, |p| {
            p.build_launch_command(&AgentLaunchParams {
                skip_permissions: true,
                ..Default::default()
            })
        });
        let cmd = cmd.unwrap_or_else(|| panic!("provider {} missing", id));
        assert!(
            !cmd.is_empty(),
            "provider {} returned empty launch command",
            id
        );
    }
}

#[test]
fn list_descriptors_serializes_to_json_array() {
    let reg = registry_with_claude();
    let descs = reg.list_descriptors();
    let json = serde_json::to_value(&descs).expect("serialize");
    assert!(json.is_array());
    assert_eq!(json.as_array().unwrap().len(), 1);
}
