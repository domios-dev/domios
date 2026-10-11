use super::*;
use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
use std::sync::Arc;

const CAPTURED_SCREEN: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../tests/fixtures/tab-respond/reasoning-submenu-0.160.0.txt"
));

fn registry() -> AgentRegistry {
    let mut manifest: PluginManifest = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../plugins/provider-codex/plugin.json"
    )))
    .unwrap();
    manifest.id = "response-fixture".into();
    let vm = PluginVm::from_source(
        "response-fixture",
        include_str!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../plugins/provider-codex/plugin.js"
        ))
        .into(),
    )
    .unwrap();
    let registry = AgentRegistry::new();
    registry.register_provider(Box::new(JsProvider::new(manifest, Arc::new(vm))));
    registry
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
