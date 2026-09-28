use super::*;
use crate::provider::AgentProvider;

#[test]
fn claude_onboarding_prompt_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let fixture = std::fs::read_to_string(
        workspace_plugins_root()
            .join("provider-claude")
            .join("tests/fixtures/onboarding/auto-environment.txt"),
    )
    .expect("onboarding fixture");
    assert_eq!(
        native.launch_onboarding_response(&fixture),
        js.launch_onboarding_response(&fixture),
        "provider implementations agree on the safe choice"
    );
    assert_eq!(native.launch_onboarding_response(&fixture), Some(b"\r".to_vec()));

    let focused_yes = fixture
        .replace("❯ 2.", "  2.")
        .replace("  1.", "❯ 1.");
    assert_eq!(
        native.launch_onboarding_response(&focused_yes),
        Some(b"j".to_vec()),
        "the host advances only one observed choice at a time"
    );
    assert_eq!(native.launch_onboarding_response(&focused_yes), js.launch_onboarding_response(&focused_yes));

    for ambiguous_focus in [
        fixture.replacen("❯ 2.", "  2.", 1),
        fixture.replacen("  1.", "❯ 1.", 1),
    ] {
        assert_eq!(native.launch_onboarding_response(&ambiguous_focus), None);
        assert_eq!(js.launch_onboarding_response(&ambiguous_focus), None);
    }

    let question = fixture.lines().next().expect("question row");
    let changed_question = fixture.replacen(question, "A different startup question?", 1);
    let mut missing_choice = fixture.lines().map(str::to_string).collect::<Vec<_>>();
    let third = missing_choice
        .iter_mut()
        .find(|line| line.trim_start().starts_with("3."))
        .expect("third option");
    *third = "  3. Later".into();
    for unsafe_screen in [changed_question, missing_choice.join("\n")] {
        assert_eq!(native.launch_onboarding_response(&unsafe_screen), None);
        assert_eq!(js.launch_onboarding_response(&unsafe_screen), None);
    }
}
