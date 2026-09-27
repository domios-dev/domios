use super::super::api_map::cli_command_to_api;
use crate::{Commands, DelayedAction};

#[test]
fn send_every_maps_recurrence_through_mesh() {
    let before = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    let cmd = Commands::SendText {
        text: Some("monitor".into()),
        tab: Some("abc123".into()),
        from_file: None,
        from_pad: None,
        stdin: false,
        actions: None,
        delay: None,
        at: None,
        every: Some("5m".into()),
        until: None,
        count: Some(3),
        require_agent: false,
        to_shell: false,
    };
    let (method, path, body) = cli_command_to_api(&cmd);
    let after = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/abc123/delayed-messages");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert_eq!(body["text"], "monitor");
    assert!(body["scheduledAtMs"].as_u64().is_some_and(|scheduled| {
        scheduled >= before.saturating_add(300_000) && scheduled <= after.saturating_add(300_000)
    }));
    assert_eq!(body["recurrence"]["everyMs"], 300_000);
    assert_eq!(body["recurrence"]["count"], 3);
    assert!(body["recurrence"]["untilMs"].is_null());
}

#[test]
fn delayed_edit_maps_recurrence_and_stop_through_mesh() {
    let until_ms = 4_102_444_800_000_u64;
    let cmd = Commands::Delayed {
        action: DelayedAction::Edit {
            id: "series-1".into(),
            text: None,
            delay: None,
            at: None,
            every: Some("15m".into()),
            until: Some(until_ms.to_string()),
            count: None,
            from_file: None,
            from_pad: None,
            stdin: false,
        },
    };
    let (method, path, body) = cli_command_to_api(&cmd);
    assert_eq!(method, "PATCH");
    assert_eq!(path, "/api/delayed-messages/series-1");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert_eq!(
        body["recurrence"],
        serde_json::json!({
            "everyMs": 900_000,
            "untilMs": until_ms,
            "count": null,
        })
    );
}

#[test]
fn mesh_one_shot_delay_to_old_peer_still_works() {
    let cmd = Commands::Delayed {
        action: DelayedAction::Create {
            text: Some("one shot".into()),
            tab: Some("tab-1".into()),
            delay: Some("1m".into()),
            at: None,
            every: None,
            until: None,
            count: None,
            from_file: None,
            from_pad: None,
            stdin: false,
        },
    };
    let (method, path, body) = cli_command_to_api(&cmd);
    assert_eq!(method, "POST");
    assert_eq!(path, "/api/tabs/tab-1/delayed-messages");
    let body: serde_json::Value = serde_json::from_str(&body.expect("body")).unwrap();
    assert!(body.get("recurrence").is_none());
}
