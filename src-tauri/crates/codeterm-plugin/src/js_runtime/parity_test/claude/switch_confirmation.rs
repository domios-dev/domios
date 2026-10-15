use super::*;

#[test]
fn switch_confirmation_prompt_native_js_parity() {
    assert!(require_plugins().is_some(), "bundled plugin is required");
    let (native, js) = pair();
    assert_eq!(native.model_switch_commands(), js.model_switch_commands());
    let screen =
        provider_fixture(js.id(), "model-switch/sonnet-confirmation.txt").replace("\r\n", "\n");
    for frame in [screen.clone(), screen.replace('\n', "\r\n")] {
        for selection in ["sonnet", "foreign"] {
            let expected = native.parse_switch_confirmation_prompt(&frame, selection);
            assert_eq!(
                js.parse_switch_confirmation_prompt(&frame, selection),
                expected
            );
            assert_eq!(expected.is_some(), selection == "sonnet");
        }
        for changed in [format!("{frame}\n❯ "), frame.replace("❯ 1.", "  1.")] {
            assert!(native
                .parse_switch_confirmation_prompt(&changed, "sonnet")
                .is_none());
            assert!(js
                .parse_switch_confirmation_prompt(&changed, "sonnet")
                .is_none());
        }
    }
    let synthetic = "Opaque header?\nOpaque context\n❯ 1. opaque high\n  2. opaque alternate";
    let prompt = native
        .parse_switch_confirmation_prompt(synthetic, "high")
        .unwrap();
    assert_eq!(
        js.parse_switch_confirmation_prompt(synthetic, "high"),
        Some(prompt)
    );
}

#[test]
fn cached_switch_confirmation_native_js_parity() {
    assert!(require_plugins().is_some(), "bundled plugin is required");
    let (native, js) = pair();
    let screen = provider_fixture(js.id(), "model-switch/cached-sonnet-confirmation.txt");
    for provider in [&native as &dyn AgentProvider, &js] {
        let prompt = provider
            .parse_switch_confirmation_prompt(&screen, "sonnet")
            .unwrap();
        assert_eq!(prompt.options.len(), 2);
        assert_eq!(
            provider.live_prompt_response(0, &screen),
            Some(vec![b"\r".to_vec()])
        );
        assert!(provider
            .parse_switch_confirmation_prompt(&screen, "opus")
            .is_none());
        assert!(provider
            .parse_switch_confirmation_prompt("composer", "sonnet")
            .is_none());
    }
}
