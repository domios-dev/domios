//! Hook event roster parity: native and JS providers declaring identical
//! rosters produce identical route registrations; an undeclared event routes
//! nowhere on either runtime; an unknown route fails registration closed at
//! install and is reported via check_integration.

use super::*;
use crate::js_runtime::{PluginManifest, PluginVm};
use crate::provider::{
    declared_state_verdict, HookEventBinding, HookIngestionRoute, IntegrationInstallOptions,
    ProviderStateVerdict,
};

fn load_roster_js(id: &str, roster_json: &str) -> JsProvider {
    use std::sync::Arc;

    let manifest_json = format!(
        r#"{{"id":"{id}","version":"0.1.0","displayName":"Roster Fixture",
                "spawn":{{"readinessStrategy":{{"kind":"fixed_delay","durationMs":0}},
                         "taskDelivery":"pty_input","verifySubmit":{{"kind":"none"}},
                         "hasHooks":true,
                         "hookEventRoster":{roster_json}}}}}"#
    );
    let manifest: PluginManifest = serde_json::from_str(&manifest_json).unwrap();
    let vm = PluginVm::from_source_with(
        id,
        "module.exports.default = {};".to_string(),
        Some(manifest_json),
    )
    .expect("roster fixture vm");
    JsProvider::new(manifest, Arc::new(vm))
}

fn roster_binding(event: &str, route: HookIngestionRoute) -> HookEventBinding {
    HookEventBinding {
        event: event.into(),
        route,
        verdict: None,
    }
}

fn state_binding(event: &str, verdict: ProviderStateVerdict) -> HookEventBinding {
    HookEventBinding {
        event: event.into(),
        route: HookIngestionRoute::SessionState,
        verdict: Some(verdict),
    }
}

struct RosterNativeStub;

impl AgentProvider for RosterNativeStub {
    fn id(&self) -> &str {
        "roster-parity-fixture"
    }
    fn display_name(&self) -> &str {
        "Roster Fixture"
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
    fn has_hooks(&self) -> bool {
        true
    }
    fn hook_event_roster(&self) -> Vec<HookEventBinding> {
        vec![
            roster_binding("Notification", HookIngestionRoute::Permission),
            roster_binding("Stop", HookIngestionRoute::Stop),
        ]
    }
}

#[test]
fn hook_event_roster_native_js_parity() {
    let native = RosterNativeStub;
    let js = load_roster_js(
        "roster-parity-fixture",
        r#"[{"event":"Notification","route":"permission"},{"event":"Stop","route":"stop"}]"#,
    );

    assert_eq!(js.hook_event_roster(), native.hook_event_roster());

    // Negative control: registration exposes exactly the declared events on
    // both runtimes, so a POST for an undeclared event name routes nowhere.
    for provider in [&native as &dyn AgentProvider, &js as &dyn AgentProvider] {
        let roster = provider.hook_event_roster();
        assert_eq!(
            roster.iter().map(|b| b.event.as_str()).collect::<Vec<_>>(),
            ["Notification", "Stop"]
        );
        assert!(!roster.iter().any(|b| b.event == "UndeclaredEvent"));
    }
}

#[test]
fn hook_event_roster_fail_closed_on_unknown_route() {
    let js = load_roster_js(
        "roster-fail-closed-fixture",
        r#"[{"event":"Stop","route":"warp_core"}]"#,
    );

    assert!(
        js.hook_event_roster().is_empty(),
        "an invalid roster must register no routes at all"
    );
    let err = js
        .install_integration(
            0,
            &IntegrationInstallOptions {
                consent: true,
                precompact_memory_export: false,
            },
        )
        .expect_err("registration must fail closed at install");
    assert!(err.contains("warp_core"), "got: {err}");
    let report = js.check_integration();
    assert_eq!(report["hooks_installed"], false);
    assert!(report["hook_event_roster_error"]
        .as_str()
        .is_some_and(|error| error.contains("warp_core")));
}

struct NativeFallbackStub {
    native: std::sync::Arc<crate::agent_claude::provider::ClaudeProvider>,
}

impl crate::host_traits::PluginProviderRegistrar for NativeFallbackStub {
    fn register_provider(&self, _: Box<dyn AgentProvider>) {}
    fn unregister_provider(&self, _: &str) -> bool {
        false
    }
    fn native_fallback(&self, id: &str) -> Option<std::sync::Arc<dyn AgentProvider>> {
        (id == "claude").then(|| self.native.clone() as std::sync::Arc<dyn AgentProvider>)
    }
}

#[test]
fn default_hook_event_roster_matches_bundled_manifests() {
    if require_plugins().is_none() {
        return;
    }
    // codex declares SessionStart + Stop as manifest data (DN-3 stage 2).
    assert_eq!(
        load_js("codex").hook_event_roster(),
        vec![
            roster_binding("SessionStart", HookIngestionRoute::SessionStart),
            roster_binding("Stop", HookIngestionRoute::Stop),
        ],
        "codex: declared roster is SessionStart + Stop"
    );
    // opencode declares session state as manifest data, keeping SessionStart.
    let opencode = load_js("opencode").hook_event_roster();
    assert_eq!(
        opencode,
        vec![
            roster_binding("SessionStart", HookIngestionRoute::SessionStart),
            state_binding("session.status", ProviderStateVerdict::Working),
            state_binding("session.idle", ProviderStateVerdict::Idle),
        ],
        "opencode: SessionStart is preserved alongside the declared state events"
    );
    assert_eq!(
        declared_state_verdict(&opencode, "session.status"),
        Some(ProviderStateVerdict::Working)
    );
    assert_eq!(
        declared_state_verdict(&opencode, "session.idle"),
        Some(ProviderStateVerdict::Idle)
    );
    assert_eq!(declared_state_verdict(&opencode, "session.deleted"), None);
    // A hookless provider registers nothing.
    assert!(load_js("cursor").hook_event_roster().is_empty());

    // The claude drop-in delegates to its displaced native provider.
    let native = std::sync::Arc::new(crate::agent_claude::provider::ClaudeProvider::new());
    let mut claude = load_js("claude");
    claude.set_registrar(std::sync::Arc::new(NativeFallbackStub {
        native: native.clone(),
    }));
    let roster = claude.hook_event_roster();
    assert_eq!(roster, native.hook_event_roster());
    assert_eq!(
        roster
            .iter()
            .map(|b| (b.event.as_str(), b.route))
            .collect::<Vec<_>>(),
        [
            ("PermissionRequest", HookIngestionRoute::Permission),
            ("Notification", HookIngestionRoute::Permission),
            ("Stop", HookIngestionRoute::Stop),
            ("PreCompact", HookIngestionRoute::MemFlush),
            ("SessionStart", HookIngestionRoute::SessionStart),
        ]
    );
}
