use super::{classify_backend_response, render_search_human, BackendOutcome};
use reqwest::StatusCode;

/// The exact body `sessions get <unknown>` receives today. Before the fix the
/// `--json` branch printed this and exited 0.
fn not_found_body() -> serde_json::Value {
    serde_json::Value::Null
}

#[test]
fn backend_4xx_is_a_failure() {
    assert!(matches!(
        classify_backend_response(StatusCode::NOT_FOUND, &not_found_body()),
        BackendOutcome::Failure(_)
    ));
}

#[test]
fn backend_5xx_is_a_failure() {
    assert!(matches!(
        classify_backend_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            &serde_json::json!({"error": "boom"})
        ),
        BackendOutcome::Failure(_)
    ));
}

#[test]
fn failure_verdict_is_identical_for_every_body_shape_at_the_same_status() {
    let bodies = [
        serde_json::Value::Null,
        serde_json::json!({}),
        serde_json::json!({"tab_id": "tab_1"}),
    ];
    for body in bodies {
        assert!(
            matches!(
                classify_backend_response(StatusCode::NOT_FOUND, &body),
                BackendOutcome::Failure(_)
            ),
            "404 must fail regardless of body: {body}"
        );
    }
}

#[test]
fn failure_message_names_the_backend_status_and_body() {
    let body = serde_json::json!({"error": "no such session"});
    let BackendOutcome::Failure(message) = classify_backend_response(StatusCode::NOT_FOUND, &body)
    else {
        panic!("404 must classify as a failure");
    };
    assert!(
        message.contains(StatusCode::NOT_FOUND.as_str()),
        "caller must be able to see the status the backend returned: {message}"
    );
    assert!(
        message.contains(&body.to_string()),
        "caller must be able to see the body the backend returned: {message}"
    );
}

#[test]
fn resume_2xx_stays_optimistic_when_the_body_carries_no_agent_evidence() {
    assert_eq!(
        classify_backend_response(StatusCode::OK, &serde_json::json!({})),
        BackendOutcome::Success
    );
}

#[test]
fn resume_2xx_is_not_downgraded_by_an_error_shaped_body() {
    let hostile = serde_json::json!({
        "ok": false,
        "error": "agent never started",
        "tab_id": serde_json::Value::Null,
        "identity": {"status": "roleless"},
    });
    assert_eq!(
        classify_backend_response(StatusCode::OK, &hostile),
        BackendOutcome::Success,
        "only the HTTP status may fail a resume; adding a body heuristic breaks optimistic success"
    );
}

#[test]
fn search_2xx_with_no_hits_is_a_success_not_a_not_found() {
    assert_eq!(
        classify_backend_response(StatusCode::OK, &serde_json::json!({"hits": []})),
        BackendOutcome::Success,
        "an empty result set is an answer, not a backend failure"
    );
}

#[test]
fn search_5xx_fails_even_though_the_body_parses_as_a_result_set() {
    assert!(
        matches!(
            classify_backend_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                &serde_json::json!({"hits": [], "error": "index unavailable"})
            ),
            BackendOutcome::Failure(_)
        ),
        "search previously printed this body and exited 0 in BOTH modes"
    );
}

#[test]
fn every_success_status_class_member_stays_success() {
    for status in [
        StatusCode::OK,
        StatusCode::CREATED,
        StatusCode::ACCEPTED,
        StatusCode::NO_CONTENT,
    ] {
        assert_eq!(
            classify_backend_response(status, &serde_json::Value::Null),
            BackendOutcome::Success,
            "{status} is a 2xx and must not be failed"
        );
    }
}

/// Warnings reach the human output even when nothing matched, so a skipped
/// provider store never reads as a clean "no match".
#[test]
fn search_human_output_carries_every_warning() {
    let body = serde_json::json!({
        "hits": [],
        "warnings": [
            {"provider": "p1", "reason": "unreadable", "sessions": 2, "message": "warn-token-a"},
            {"provider": "p2", "reason": "over_byte_cap", "sessions": 1, "message": "warn-token-b"}
        ]
    });
    let out = render_search_human("q", &body);
    assert_eq!(out.matches("warn-token-a").count(), 1, "{out}");
    assert_eq!(out.matches("warn-token-b").count(), 1, "{out}");
}

#[test]
fn search_human_resume_hint_is_the_provider_agnostic_command() {
    let body = serde_json::json!({
        "hits": [{
            "meta": {"provider": "any-provider", "session_id": "ses_token", "file_path": "/x"},
            "snippets": [{"role": "assistant", "text": "snippet-token"}],
            "match_count": 1
        }],
        "warnings": []
    });
    let out = render_search_human("q", &body);
    assert!(out.contains("codeterm sessions resume ses_token"), "{out}");
    assert!(out.contains("[any-provider] snippet-token"), "{out}");
}
