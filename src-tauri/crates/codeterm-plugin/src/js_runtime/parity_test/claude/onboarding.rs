use super::*;
use crate::provider::AgentProvider;

#[test]
fn claude_onboarding_prompt_native_js_parity() {
    if require_plugins().is_none() {
        return;
    }
    let (native, js) = pair();
    let fixtures = workspace_plugins_root()
        .join("provider-claude")
        .join("tests/fixtures/onboarding");
    for file in [
        "auto-environment-captured.txt",
        "auto-environment-variant.txt",
        "auto-environment-not-now.txt",
        "auto-environment-boxed-curly.txt",
    ] {
        let screen = std::fs::read_to_string(fixtures.join(file)).expect("onboarding fixture");
        assert_eq!(
            native.parse_prompt(&screen),
            js.parse_prompt(&screen),
            "prompt parse parity for {file}"
        );
        let native_response = native.launch_onboarding_response(&screen);
        assert_eq!(
            native_response,
            js.launch_onboarding_response(&screen),
            "response parity for {file}"
        );
        let response = native_response.expect("classified onboarding frame");
        let expected_step = if file == "auto-environment-captured.txt" {
            b"j".as_slice()
        } else {
            b"\r".as_slice()
        };
        assert_eq!(
            response.step.as_slice(),
            expected_step,
            "safe next step for {file}"
        );
    }

    for file in [
        "auto-environment-no-space.txt",
        "auto-environment-empty-label.txt",
    ] {
        let screen = std::fs::read_to_string(fixtures.join(file)).expect("onboarding fixture");
        assert_eq!(
            native.parse_prompt(&screen),
            js.parse_prompt(&screen),
            "fallback parse parity for {file}"
        );
        assert_eq!(
            native.launch_onboarding_response(&screen),
            None,
            "unsafe option syntax refused for {file}"
        );
        assert_eq!(
            js.launch_onboarding_response(&screen),
            None,
            "unsafe option syntax refused for {file}"
        );
    }

    let capture = std::fs::read_to_string(fixtures.join("auto-environment-captured.txt"))
        .expect("captured onboarding frame");
    let first_option = capture.find("1. Yes").expect("first choice");
    let focus = capture[..first_option]
        .rfind('❯')
        .expect("captured focus marker");
    let ambiguous = format!(
        "{}{}",
        &capture[..focus],
        &capture[focus + '❯'.len_utf8()..]
    );
    assert_eq!(native.launch_onboarding_response(&ambiguous), None);
    assert_eq!(js.launch_onboarding_response(&ambiguous), None);

    let question = capture
        .lines()
        .find(|line| line.trim_end().ends_with('?'))
        .expect("question row");
    let changed_question = capture.replacen(question, "A different startup question?", 1);
    assert_eq!(native.launch_onboarding_response(&changed_question), None);
    assert_eq!(js.launch_onboarding_response(&changed_question), None);
}
