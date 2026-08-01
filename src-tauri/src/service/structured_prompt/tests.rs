//! Store-level tests for the structured-prompt host module. The SSE parsing is
//! covered in `opencode_sse::tests`; the live round-trip is verified by the
//! orchestrator against a real opencode server.

use super::opencode_sse::{PromptKind, ResponseRoute};
use super::*;
use crate::multiplexer::types::PendingPrompt;

fn dummy_prompt(q: &str) -> PendingPrompt {
    PendingPrompt {
        question: q.to_string(),
        options: Vec::new(),
        sections: None,
        multi_select: Some(false),
        submit_available: Some(true),
        selected_values: Some(Vec::new()),
    }
}

#[test]
fn endpoint_and_active_round_trip_per_tab() {
    let tab = "test-tab-sp-roundtrip";
    register_endpoint(tab, "http://127.0.0.1:55555".into(), "/work".into());
    assert!(pending_for_tab(tab).is_none(), "no prompt yet");

    ACTIVE.lock().insert(
        tab.to_string(),
        Active {
            prompt: dummy_prompt("Pick one"),
            route: ResponseRoute {
                session_id: "ses_1".into(),
                request_id: "que_1".into(),
                kind: PromptKind::Question,
                option_labels: vec!["A".into(), "B".into()],
            },
        },
    );
    assert_eq!(pending_for_tab(tab).unwrap().question, "Pick one");
    assert_eq!(route_for_tab(tab).unwrap().request_id, "que_1");

    clear_tab(tab);
    assert!(pending_for_tab(tab).is_none());
    assert!(route_for_tab(tab).is_none());
    assert!(ENDPOINTS.lock().get(tab).is_none());
}

#[test]
fn allocate_server_returns_loopback_port() {
    let server = allocate_server().expect("port allocated");
    assert_eq!(server.hostname, "127.0.0.1");
    assert!(server.port > 0);
}

#[test]
fn pending_for_unknown_tab_is_none() {
    assert!(pending_for_tab("test-tab-sp-never-registered").is_none());
}
