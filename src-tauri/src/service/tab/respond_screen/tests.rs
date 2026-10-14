use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;

const CAPTURED_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/tab-respond/reasoning-submenu-0.160.0.txt"
));

fn bundled_registry(manifest: &str, source: &str) -> AgentRegistry {
    let mut manifest: PluginManifest = serde_json::from_str(manifest).unwrap();
    manifest.id = "response-fixture".into();
    let vm = PluginVm::from_source("response-fixture", source.into()).unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
    registry
}

fn registry() -> AgentRegistry {
    bundled_registry(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-codex/plugin.json"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-codex/plugin.js"
        )),
    )
}

fn respond(
    registry: &AgentRegistry,
    screen: &str,
    identity: Option<&PromptIdentity>,
) -> Result<Vec<Vec<u8>>, RespondPromptOutcome> {
    response_steps(
        registry,
        Some("response-fixture"),
        screen,
        1,
        identity,
        || panic!("the provider must declare the response keys"),
    )
}

fn identity(registry: &AgentRegistry) -> PromptIdentity {
    let (prompt, kind) = registry
        .parse_interactive_prompt("response-fixture", CAPTURED_SCREEN)
        .expect("captured menu is classified and parsed");
    assert_eq!(prompt.options.len(), 5);
    PromptIdentity::new(kind, &prompt)
}

#[test]
fn captured_submenu_option_two_writes_only_declared_enter() {
    let registry = registry();
    let identity = identity(&registry);
    assert_eq!(
        respond(&registry, CAPTURED_SCREEN, None),
        Ok(vec![b"\r".to_vec()])
    );
    assert_eq!(
        respond(&registry, CAPTURED_SCREEN, Some(&identity)),
        Ok(vec![b"\r".to_vec()])
    );

    let mut state = crate::pty::screen_state::ScreenState::new();
    state.resize("fixture", 120, 24);
    state.process("fixture", CAPTURED_SCREEN.replace('\n', "\r\n").as_bytes());
    let rendered = state.get_screen_text("fixture").unwrap();
    assert_eq!(
        respond(&registry, &rendered, None),
        Ok(vec![b"\r".to_vec()])
    );
}

#[test]
fn vanished_submenu_refuses_response_even_with_saved_identity() {
    let registry = registry();
    let identity = identity(&registry);
    for screen in [
        String::new(),
        format!("{CAPTURED_SCREEN}\n• Working (esc to interrupt)\n›"),
    ] {
        assert_eq!(
            respond(&registry, &screen, None),
            Err(RespondPromptOutcome::PromptNotOnScreen)
        );
        assert_eq!(
            respond(&registry, &screen, Some(&identity)),
            Err(RespondPromptOutcome::StalePrompt)
        );
    }
}

#[test]
fn changed_submenu_refuses_saved_option_identity() {
    let registry = registry();
    let identity = identity(&registry);
    let changed = CAPTURED_SCREEN.replace("Medium (current)", "Changed (current)");
    assert_eq!(
        respond(&registry, &changed, Some(&identity)),
        Err(RespondPromptOutcome::StalePrompt)
    );
}

const CAPTURED_TRUST_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/tab-respond/directory-trust-matrix-1147.txt"
));

const OUTSIDE_DIRECTORY_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/tab-respond/outside-directory-permission.txt"
));

fn trust_registry() -> AgentRegistry {
    bundled_registry(
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-claude/plugin.json"
        )),
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-claude/plugin.js"
        )),
    )
}

#[test]
fn captured_directory_trust_admits_option_two_once() {
    let registry = trust_registry();
    let (prompt, kind) = registry
        .parse_interactive_prompt("response-fixture", CAPTURED_TRUST_SCREEN)
        .expect("captured directory trust capability");
    assert_eq!(kind, PromptKind::DirectoryTrust);
    assert_eq!(prompt.options.len(), 2);
    let identity = PromptIdentity::new(kind, &prompt);
    for expected in [None, Some(&identity)] {
        assert_eq!(
            respond(&registry, CAPTURED_TRUST_SCREEN, expected),
            Ok(vec![b"j".to_vec(), b"\r".to_vec()])
        );
    }
}

#[test]
fn outside_directory_permission_parses_three_options_and_responds() {
    let registry = trust_registry();
    let (prompt, kind) = registry
        .parse_interactive_prompt("response-fixture", OUTSIDE_DIRECTORY_SCREEN)
        .expect("outside-directory permission is a live provider prompt");
    assert_eq!(kind, PromptKind::Provider);
    assert_eq!(
        prompt
            .options
            .iter()
            .map(|option| option.label.as_str())
            .collect::<Vec<_>>(),
        [
            "Yes",
            "Yes, and always allow access to D:\\outside-target from this project",
            "No",
        ]
    );
    let steps = response_steps(
        &registry,
        Some("response-fixture"),
        OUTSIDE_DIRECTORY_SCREEN,
        0,
        None,
        || panic!("the provider must declare the response keys"),
    );
    assert_eq!(steps, Ok(vec![b"\r".to_vec()]));

    let mut state = crate::pty::screen_state::ScreenState::new();
    state.resize("fixture", 120, 24);
    state.process(
        "fixture",
        OUTSIDE_DIRECTORY_SCREEN.replace('\n', "\r\n").as_bytes(),
    );
    let rendered = state.get_screen_text("fixture").unwrap();
    assert_eq!(
        response_steps(
            &registry,
            Some("response-fixture"),
            &rendered,
            0,
            None,
            || panic!("the provider must declare the response keys"),
        ),
        Ok(vec![b"\r".to_vec()])
    );
}

#[test]
fn stale_or_absent_directory_trust_refuses_response() {
    let registry = trust_registry();
    let (prompt, kind) = registry
        .parse_interactive_prompt("response-fixture", CAPTURED_TRUST_SCREEN)
        .expect("captured directory trust capability");
    let identity = PromptIdentity::new(kind, &prompt);
    assert_eq!(
        respond(&registry, "", None),
        Err(RespondPromptOutcome::PromptNotOnScreen)
    );
    assert_eq!(
        respond(&registry, "", Some(&identity)),
        Err(RespondPromptOutcome::StalePrompt)
    );
    let changed = CAPTURED_TRUST_SCREEN.replace("No, exit", "opaque-decline");
    assert_eq!(
        respond(&registry, &changed, Some(&identity)),
        Err(RespondPromptOutcome::StalePrompt)
    );
}
