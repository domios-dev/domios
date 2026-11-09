use super::*;
use serde_json::{json, Value};

#[test]
fn goal_snapshots_preserve_unsupported_and_nullable_intent_shapes() {
    let snapshots = [
        GoalSnapshot::Unsupported { supported: false },
        GoalSnapshot::Supported {
            supported: true,
            binding: GoalBinding {
                thread_id: "thread".into(),
                binding_nonce: "nonce".into(),
                epoch: 1,
            },
            intent: None,
            actions: vec![crate::agent_runtime::RuntimeGoalAction::Read],
        },
    ];
    assert_eq!(
        serde_json::to_value(snapshots).unwrap(),
        json!([
            {"supported": false},
            {"supported": true, "binding": {"threadId": "thread", "bindingNonce": "nonce", "epoch": 1}, "intent": null, "actions": ["read"]},
        ])
    );
}

#[test]
fn stored_latest_omits_runtime_readings_and_keeps_plugin_details() {
    let snapshot = serde_json::from_value(json!({
        "id": 1, "provider": "fixture", "captured_at": 10, "parsed_ok": true,
        "usage_details": {"custom": [1, "opaque"]},
    }))
    .unwrap();
    let value = serde_json::to_value(UsageLatestResponse::Stored {
        snapshot,
        runtime_usage: None,
    })
    .unwrap();
    assert_eq!(
        (
            value.get("runtime_usage").cloned(),
            value.get("account_id").cloned()
        ),
        (None, Some(Value::Null))
    );
    assert_eq!(value["usage_details"], json!({"custom": [1, "opaque"]}));
}
