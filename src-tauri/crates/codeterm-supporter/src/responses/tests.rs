use super::*;
use serde_json::json;

#[test]
fn responses_preserve_nulls_and_omitted_fields() {
    let token = SupporterTokenStatus {
        adapter: "fixture".into(),
        token_mode: TokenMode::None,
        present: false,
        path: PathBuf::from("fixture.json"),
        mode_ok: None,
    };
    assert_eq!(
        json!(token),
        json!({ "adapter": "fixture", "token_mode": "none", "present": false,
        "path": "fixture.json", "mode_ok": null })
    );
    let adapter = SupporterAdapter {
        id: "fixture".into(),
        token_mode: TokenMode::None,
        primitives: Default::default(),
        models: Vec::new(),
        token: None,
        pricing: None,
        metadata: Default::default(),
    };
    assert!(json!(adapter).get("token").is_none());
    assert_eq!(json!(adapter)["pricing"], Value::Null);
    let connection = SupporterConnectionRow {
        adapter: "fixture".into(),
        model: None,
        active: false,
        token_mode: None,
        token: SupporterConnectionToken {
            present: false,
            mode_ok: None,
        },
        pricing: None,
        metadata: None,
        error: None,
    };
    assert_eq!(
        json!(connection),
        json!({ "adapter": "fixture", "model": null, "active": false,
        "token_mode": null, "token": { "present": false, "mode_ok": null },
        "pricing": null, "metadata": null, "error": null })
    );
    assert_eq!(
        json!(SupporterMutation::Model {
            adapter: "fixture".into(),
            model: None
        }),
        json!({ "adapter": "fixture", "model": null })
    );
    assert_eq!(
        json!(SupporterMutation::Disconnected {
            adapter: "fixture".into(),
            removed: true,
            active: None
        }),
        json!({ "adapter": "fixture", "removed": true, "active": null })
    );
    let dir = tempfile::tempdir().unwrap();
    let runtime = crate::SupporterRuntime::new(dir.path().into());
    let status = SupporterStatus {
        state: SupporterState::NotConnected,
        token_mode: TokenMode::None,
        adapter: None,
        model: None,
        deadline_ms: 3000,
        connected: false,
        activity: runtime.status(0),
        observation_scheduler: SupporterObservationScheduler {
            questions_per_event: 6,
            global_in_flight: 4,
            per_tab_in_flight: 1,
            queue_limit: 32,
            active: 0,
            queued: 0,
            coalesced: 0,
            dropped: 0,
        },
        pricing: None,
        spend: runtime.spend_status("fixture"),
        token,
        adapter_registered: false,
        adapters: vec![adapter],
        connections: vec![connection],
    };
    let value = json!(status);
    for field in ["adapter", "model", "pricing", "last_activity"] {
        assert_eq!(value.get(field), Some(&Value::Null), "{field}");
    }
    assert!(value.get("activity").is_none());
    assert_eq!(value["budget"].get("free_at"), Some(&Value::Null));
    assert_eq!(value["spend"].get("next_probe_at"), Some(&Value::Null));
}
