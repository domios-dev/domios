use super::*;
use codeterm_types::agent::provider_signals::ApiErrorKind;

#[test]
fn parity_provider_access_error_fixtures() {
    if require_plugins().is_none() {
        return;
    }
    let native = crate::agent_claude::ClaudeProvider::new();
    let claude = load_js("claude");
    let codex = load_js("codex");
    let fixtures: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            workspace_plugins_root().join("provider-shared/access-errors.json"),
        )
        .unwrap(),
    )
    .unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let claude_record = &fixture["claude"];
        let expected: Option<ApiErrorKind> =
            serde_json::from_value(fixture["expected"].clone()).unwrap();
        let observations = [
            native.parse_session_observations(claude_record),
            claude.parse_session_observations(claude_record),
            codex.parse_session_observations(&fixture["codex"]),
        ];
        assert_eq!(observations[0], observations[1]);
        for observation in observations {
            if fixture["absent"] == true {
                assert!(observation.is_none());
            } else {
                let observation = observation.unwrap();
                assert_eq!(observation.api_error, expected);
                assert_eq!(observation.clears_api_error, fixture["clears"] == true);
            }
        }
    }
}

#[test]
fn parity_claude_fermi_org_disabled_access_fixture() {
    if require_plugins().is_none() {
        return;
    }
    let native = crate::agent_claude::ClaudeProvider::new();
    let js = load_js("claude");
    let fixture = std::fs::read_to_string(
        workspace_plugins_root().join("provider-claude/tests/fixtures/access-lost/fermi.jsonl"),
    )
    .unwrap();
    let mut refusals = 0;
    for line in fixture.lines() {
        let record: serde_json::Value = serde_json::from_str(line).unwrap();
        let observation = native.parse_session_observations(&record);
        assert_eq!(observation, js.parse_session_observations(&record));
        if record["isApiErrorMessage"] == true {
            refusals += 1;
            assert_eq!(
                observation.unwrap().api_error,
                Some(ApiErrorKind::Authentication)
            );
        }
    }
    assert_eq!(refusals, 1);
}
