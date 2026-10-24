use super::*;
use crate::surface::runtime::{SurfaceNotice, SurfaceState};
use serde_json::json;

#[test]
fn stdin_secret_is_redacted_before_snapshot_truncation_and_from_events_and_inbox() {
    use crate::surface::runtime::SurfaceResume;
    use codeterm_browser::{
        snapshot::{build_snapshot, parse_full_ax_tree},
        BrowserHostConfig,
    };
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let runtime = super::super::BrowserRuntime::new(
        BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab-id",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume::default(),
    );
    let secret = "opaque-λ-body".repeat(12);
    runtime.secrets.lock().push(secret.clone());
    let mut tree = json!({"nodes":[{"nodeId":"1", "role":{"value":"textbox"}, "name":{"value":"Password"}, "value":{"value":secret}, "backendDOMNodeId":1}]});
    runtime.redact_value(&mut tree);
    let snapshot = build_snapshot(&parse_full_ax_tree(&tree).unwrap(), 20);
    assert!(snapshot.text.contains("REDACTED"));
    assert!(!snapshot.text.contains("opaque-λ-body"));
    let outcome = runtime.annotation_outcome("s1".into(), format!("value: {secret}"));
    assert!(!serde_json::to_string(&outcome.result)
        .unwrap()
        .contains("opaque-λ-body"));
    assert_eq!(runtime.inbox.lock().take(false), "value: REDACTED");
    let SurfaceEvent::State(state) = runtime.redact_event(SurfaceEvent::State(SurfaceState {
        title: secret.clone(),
        activity: secret.clone(),
        ..Default::default()
    })) else {
        panic!("state");
    };
    assert_eq!(state.title, "REDACTED");
    assert_eq!(state.activity, "REDACTED");
    let SurfaceEvent::Notice(notice) =
        runtime.redact_event(SurfaceEvent::Notice(SurfaceNotice::error(secret)))
    else {
        panic!("notice");
    };
    assert_eq!(notice.text, "REDACTED");
}

#[test]
fn browser_redaction_matches_plugin_fixtures_and_is_idempotent() {
    let cases: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../plugins/browser/tests/redaction-cases.json"
    )))
    .unwrap();
    for case in cases.as_array().unwrap() {
        let expected = case["expected"].as_str().unwrap();
        assert_eq!(text(case["input"].as_str().unwrap()), expected);
        assert_eq!(text(expected), expected);
    }
}

#[test]
fn browser_redaction_covers_nested_results_and_annotation_delivery() {
    let url = "https://app.test/#token=opaque";
    let redacted = "https://app.test/#token=REDACTED";
    let result = outcome(SurfaceCallOutcome {
        result: json!({ "url": url, "nested": [url, 1, null] }),
        effects: vec![SurfaceEffect::DeliverAnnotation {
            id: "g1".into(),
            markdown: format!("Page: {url}"),
        }],
    });
    assert_eq!(
        result.result,
        json!({ "url": redacted, "nested": [redacted, 1, null] })
    );
    assert_eq!(
        result.effects,
        vec![SurfaceEffect::DeliverAnnotation {
            id: "g1".into(),
            markdown: format!("Page: {redacted}"),
        }]
    );
}

#[test]
fn browser_redaction_covers_state_and_notice_events_without_changing_live_state() {
    let state = SurfaceState {
        url: "https://app.test/?password=opaque".into(),
        activity: "goto https://app.test/#key=opaque".into(),
        ..Default::default()
    };
    let SurfaceEvent::State(redacted) = event(SurfaceEvent::State(state.clone())) else {
        panic!("expected state event");
    };
    assert_eq!(redacted.url, "https://app.test/?password=REDACTED");
    assert_eq!(redacted.activity, "goto https://app.test/#key=REDACTED");
    assert_eq!(state.url, "https://app.test/?password=opaque");
    let SurfaceEvent::Notice(notice) = event(SurfaceEvent::Notice(SurfaceNotice::preview_paused(
        "failed https://app.test/#secret=opaque",
    ))) else {
        panic!("expected notice event");
    };
    assert_eq!(
        notice.detail.as_deref(),
        Some("failed https://app.test/#secret=REDACTED")
    );
}

#[test]
fn browser_redaction_keeps_annotation_inbox_credentials_out_of_agent_delivery() {
    use super::super::BrowserRuntime;
    use crate::surface::runtime::SurfaceResume;
    use codeterm_browser::BrowserHostConfig;
    use std::sync::Arc;

    let root = tempfile::tempdir().unwrap();
    let runtime = BrowserRuntime::new(
        BrowserHostConfig::under_data_dir(root.path()),
        "Agent",
        "tab-id",
        Arc::new(tokio::sync::OnceCell::new()),
        SurfaceResume::default(),
    );
    let result =
        runtime.annotation_outcome("g1".into(), "Page: https://app.test/#token=opaque".into());
    assert_eq!(
        result.result["markdown"],
        "Page: https://app.test/#token=REDACTED"
    );
    assert_eq!(
        runtime.inbox.lock().take(false),
        "Page: https://app.test/#token=REDACTED"
    );
}
