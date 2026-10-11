use super::*;

#[test]
fn parity_update_prompt_safe_choice() {
    assert!(require_plugins().is_some(), "bundled plugins are required");
    let fixtures = [
        (
            vec![("apply", "Update now"), ("skip", "Skip")],
            Some("skip"),
        ),
        (vec![("1", "Update now"), ("2", "Later")], Some("2")),
        (
            vec![("apply", "Update now"), ("postpone", "Postpone")],
            Some("postpone"),
        ),
        (vec![("1", "Yes"), ("2", "No")], None),
        (vec![("1", "Update now"), ("2", "Cancel")], None),
        (
            vec![("1", "Update now"), ("2", "Skip"), ("3", "Later")],
            None,
        ),
        (vec![("1", "Update now"), ("1", "Skip")], None),
        (vec![("1", "Update later"), ("2", "Update now")], None),
    ];
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    for (options, expected) in fixtures {
        let prompt = crate::prompt::build_prompt(
            "fixture-question".into(),
            options
                .into_iter()
                .map(|(v, l)| (l.into(), v.into()))
                .collect(),
        );
        assert_eq!(
            native.update_prompt_safe_choice(&prompt).as_deref(),
            expected
        );
        for id in ["claude", "codex", "opencode"] {
            assert_eq!(
                load_js(id).update_prompt_safe_choice(&prompt),
                native.update_prompt_safe_choice(&prompt),
                "{id}"
            );
        }
    }
}

#[test]
fn parity_update_prompt_safe_choice_captured_menu() {
    let screen = std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
        "../../../plugins/provider-codex/tests/fixtures/directory-trust/codex-update-available.txt",
    ))
    .unwrap();
    let prompt = load_js("codex")
        .parse_prompt(&screen)
        .expect("captured menu parses");
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    assert_eq!(
        native.update_prompt_safe_choice(&prompt).as_deref(),
        Some("2")
    );
    for id in ["claude", "codex", "opencode"] {
        assert_eq!(
            load_js(id).update_prompt_safe_choice(&prompt),
            native.update_prompt_safe_choice(&prompt)
        );
    }
}

#[test]
fn parity_update_prompt_availability() {
    assert!(require_plugins().is_some(), "bundled plugins are required");
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let fixtures = [
        ("header 1.2.3 -> 2.3.4", Some(("1.2.3", "2.3.4"))),
        ("header v1.2.3 → v2.3.4", Some(("1.2.3", "2.3.4"))),
        ("header 1.2.3 -> 1.2.3", None),
        ("header 1.2 -> 2.3", None),
        ("header 1.2.3 -> 2.3.4\nother 3.4.5 -> 4.5.6", None),
        ("opaque frame", None),
    ];
    for (screen, expected) in fixtures {
        let version = native.update_prompt_availability(screen);
        assert_eq!(
            version
                .as_ref()
                .map(|v| (v.current_version.as_str(), v.latest_version.as_str())),
            expected
        );
        for id in ["claude", "codex", "opencode"] {
            let actual = load_js(id).update_prompt_availability(screen);
            assert_eq!(
                serde_json::to_value(actual).unwrap(),
                serde_json::to_value(&version).unwrap(),
                "{id}"
            );
        }
    }
    let js = load_inline_js("fixture", "module.exports.default = { updatePromptAvailability: () => ({currentVersion:'8.1.0', latestVersion:'8.2.0'}) };");
    assert_eq!(
        js.update_prompt_availability("opaque")
            .unwrap()
            .latest_version,
        "8.2.0"
    );
}

struct NativeUpdateFixture {
    id: &'static str,
    command: &'static str,
}

impl AgentProvider for NativeUpdateFixture {
    fn id(&self) -> &str {
        self.id
    }
    fn display_name(&self) -> &str {
        self.id
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
    fn session_file_path(&self, _: &str, _: &str) -> Option<PathBuf> {
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
    fn update_command(&self, platform: &str) -> Option<String> {
        matches!(platform, "macos" | "linux" | "windows").then(|| self.command.into())
    }
}

#[test]
fn parity_update_command_for_provider_fixtures() {
    assert!(require_plugins().is_some(), "bundled plugins are required");
    for (id, command) in [
        ("claude", "claude update"),
        ("codex", "codex update"),
        ("opencode", "opencode upgrade {version}"),
    ] {
        let native = NativeUpdateFixture { id, command };
        let js = load_js(id);
        for platform in ["macos", "linux", "windows", "unknown"] {
            assert_eq!(
                js.update_command(platform),
                native.update_command(platform),
                "{id}/{platform}"
            );
        }
    }
}

#[test]
fn parity_update_prompt_availability_captured_menu() {
    let screen = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-codex/tests/fixtures/directory-trust/codex-update-available.txt"),
    )
    .unwrap();
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let expected = native.update_prompt_availability(&screen).unwrap();
    assert_eq!(expected.current_version, "0.155.1");
    assert_eq!(expected.latest_version, "0.157.0");
    for id in ["claude", "codex", "opencode"] {
        assert_eq!(
            load_js(id).update_prompt_availability(&screen),
            Some(expected.clone())
        );
    }
    let undeclared = load_inline_js(
        "fixture",
        "module.exports.default = { updatePromptAvailability: () => null };",
    );
    assert_eq!(undeclared.update_prompt_availability(&screen), None);
}

#[test]
fn parity_live_prompt_response_abstains_without_explicit_valid_steps() {
    let screen = "Fixture selection\n❯ 1. Update\n 2. Skip\nEnter to confirm\n";
    let native = crate::agent_claude::provider::ClaudeProvider::new();
    let js = load_js("claude");
    assert_eq!(
        native.live_prompt_response(1, screen),
        js.live_prompt_response(1, screen)
    );
    assert!(native.live_prompt_response(1, screen).is_some());
    for source in [
        "module.exports.default = {};",
        "module.exports.default = { buildPromptResponse() { throw new Error('fixture'); } };",
        "module.exports.default = { buildPromptResponse() { return ['not-base64']; } };",
        "module.exports.default = { buildPromptResponse() { return []; } };",
        "module.exports.default = { buildPromptResponse() { return ['']; } };",
    ] {
        let provider = load_inline_js("live-response-fixture", source);
        assert_eq!(provider.live_prompt_response(1, screen), None);
    }
    for id in ["codex", "opencode"] {
        let provider = load_js(id);
        assert_eq!(provider.launch_onboarding_safe_choice(screen), None);
        assert!(provider.live_prompt_response(1, screen).is_some());
    }
}

#[test]
fn parity_update_check_existing_capability_preserves_target_versions() {
    let expected = crate::provider::ProviderUpdateAvailability {
        current_version: "1.18.35".into(),
        latest_version: "1.19.8".into(),
    };
    let js = load_inline_js("fixture", "module.exports.default = { checkForUpdate: () => ({currentVersion:'1.18.35', latestVersion:'1.19.8'}) };");
    assert!(js.supports_update_check());
    assert_eq!(
        serde_json::to_value(js.check_for_update()).unwrap(),
        serde_json::to_value(Some(expected)).unwrap()
    );
    for id in ["claude", "codex", "opencode"] {
        assert!(load_js(id).supports_update_check(), "{id}");
    }
    for platform in ["macos", "linux", "windows"] {
        assert!(load_js("opencode")
            .update_command(platform)
            .unwrap()
            .contains("{version}"));
    }
}
