use super::*;
use serde_json::json;

#[test]
fn snapshot_events_preserve_all_three_existing_shapes() {
    let events = [
        UsageSnapshotEvent::Runtime {
            provider: "fixture",
            account_id: None,
        },
        UsageSnapshotEvent::Snapshot {
            provider: "fixture",
            account_id: None,
            snapshot_id: 7,
        },
        UsageSnapshotEvent::Billing {
            provider: "fixture",
            account_id: None,
            tab_id: "tab",
            captured_at: 10,
        },
    ];
    assert_eq!(
        serde_json::to_value(events).unwrap(),
        json!([
            {"provider": "fixture", "account_id": null},
            {"provider": "fixture", "account_id": null, "snapshot_id": 7},
            {"provider": "fixture", "account_id": null, "tab_id": "tab", "captured_at": 10},
        ])
    );
}
