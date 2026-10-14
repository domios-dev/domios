use super::*;

#[test]
fn switch_confirmation_prompt_native_js_parity() {
    assert!(require_plugins().is_some(), "bundled plugin is required");
    let (native, js) = pair();
    assert_eq!(native.model_switch_commands(), js.model_switch_commands());
    let screen = include_str!("../../../../../../../plugins/provider-claude/tests/fixtures/model-switch/sonnet-confirmation.txt")
        .replace("\r\n", "\n");
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
