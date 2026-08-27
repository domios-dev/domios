//! PermissionRequest payload ingestion and its binding-nonce scoping.
use super::*;
use crate::agent::permission_request::parse_permission_request;
use crate::agent::prompt::{PromptDecision, PromptDetector};

fn agent_tab() -> (MuxState, String) {
    let (state, tab_id) = MultiplexerState::new();
    let mux: MuxState = Arc::new(TimedRwLock::new("mux-test", state));
    {
        let mut guard = mux.write();
        let tab = guard.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("fixture-hooks".into());
    }
    (mux, tab_id)
}

#[test]
fn permission_request_foreign_binding_nonce_is_rejected_and_affects_no_tab() {
    let (mux, tab_id) = agent_tab();
    {
        let mut guard = mux.write();
        let tab = guard.get_tab_mut(&tab_id).unwrap();
        tab.session_binding_nonce = Some("launch-nonce".into());
    }

    assert!(!binding_nonce_scoped(&mux, &tab_id, "foreign-nonce"));
    // A report the tab did not launch is refused before anything is ingested.
    assert!(binding_nonce_scoped(&mux, &tab_id, "launch-nonce"));
}

#[test]
fn permission_request_nonceless_tab_accepts_any_reported_nonce() {
    let (mux, tab_id) = agent_tab();
    assert!(binding_nonce_scoped(&mux, &tab_id, "whatever"));
}

#[test]
fn permission_request_payload_lands_intact_from_the_hook_body() {
    // Round-trip through exactly what the hook posts: the values the test
    // supplies must come back out of the detector unchanged.
    let posted = serde_json::json!({
        "tool_name": "Bash",
        "tool_input": {"command": "git status --short", "timeout": 5000},
        "permission_suggestions": [
            {"behavior": "allow", "label": "Allow Bash(git status:*)"},
            {"behavior": "deny", "label": "Deny once"}
        ],
        "binding_nonce": "launch-nonce"
    });
    let signal = parse_permission_request(&posted).expect("hook body parses");

    let mut detector = PromptDetector::new();
    let decision = detector.hook_detected_permission_request("tab-a", signal);
    match decision {
        PromptDecision::NewPrompt { prompt, .. } => {
            assert_eq!(prompt.question, "Bash");
            assert_eq!(
                prompt
                    .options
                    .iter()
                    .map(|o| o.label.as_str())
                    .collect::<Vec<_>>(),
                vec!["Allow Bash(git status:*)", "Deny once"]
            );
        }
        other => panic!("expected a surfaced prompt, got {other:?}"),
    }

    let stored = detector.permission_request("tab-a").expect("retained");
    assert_eq!(stored.tool_name, "Bash");
    assert_eq!(stored.tool_input, posted["tool_input"]);
    assert_eq!(
        stored.permission_suggestions,
        posted["permission_suggestions"].as_array().unwrap().clone()
    );
}

#[test]
fn permission_request_claude_roster_declares_it_against_the_permission_route() {
    let roster = crate::plugin::agent_claude::ClaudeProvider::new().hook_event_roster();
    let permission_events: Vec<&str> = roster
        .iter()
        .filter(|binding| binding.route == HookIngestionRoute::Permission)
        .map(|binding| binding.event.as_str())
        .collect();
    assert!(
        permission_events.contains(&"PermissionRequest"),
        "PermissionRequest must be declared: {permission_events:?}"
    );
    assert!(
        permission_events.contains(&"Notification"),
        "the late fallback stays declared: {permission_events:?}"
    );
    // No new route was invented for it — it lands on the one that existed.
    assert!(!roster
        .iter()
        .any(|binding| binding.event == "PermissionRequest" && binding.verdict.is_some()));
}
