use super::*;
use crate::service::agent_runtime::{engine, events::RuntimeEvent};
use codeterm_types::agent::chat_types::ChatMsg;

#[test]
fn parses_documented_statusline_quota_and_fails_closed_on_malformed() {
    let now = 1_795_280_000_000_u64;
    let payload = json!({
        "plan_tier": "Pro",
        "quota": {
            "bucket-pro": {
                "remaining_fraction": 0.25,
                "reset_time": "2026-11-22T01:00:00Z",
                "reset_in_seconds": 3600
            },
            "bucket-fast": {
                "remaining_fraction": 0.9,
                "reset_in_seconds": 600
            },
            "disabled-bucket": {
                "remaining_fraction": 0.0,
                "disabled": true
            },
            "amount-only": {
                "remaining_amount": 42
            }
        }
    });
    let parsed = parse_statusline_quota(&payload, now).unwrap();
    assert_eq!(parsed["title"], "Account quota (Pro)");
    let limits = parsed["limits"].as_array().unwrap();
    assert_eq!(limits.len(), 2);
    assert_eq!(limits[0]["label"], "bucket-pro");
    assert_eq!(limits[0]["used_percent"], 75.0);
    assert_eq!(
        limits[0]["resets_at"],
        chrono::DateTime::parse_from_rfc3339("2026-11-22T01:00:00Z")
            .unwrap()
            .timestamp_millis()
    );
    assert_eq!(limits[1]["label"], "bucket-fast");
    assert_eq!(limits[1]["used_percent"], 10.0);
    assert_eq!(limits[1]["resets_at"], (now + 600_000) as i64);

    for bad in [
        json!({}),
        json!({"quota": []}),
        json!({"quota": {"b": {"remaining_fraction": 1.5}}}),
        json!({"quota": {"b": {"remaining_fraction": -0.1}}}),
        json!({"quota": {"b": {"remaining_fraction": 0.5, "reset_in_seconds": -5}}}),
        json!({"quota": {"b": {"remaining_fraction": 0.5, "disabled": "yes"}}}),
        json!({"quota": {"bad\nkey": {"remaining_fraction": 0.5}}}),
    ] {
        assert!(
            parse_statusline_quota(&bad, now).is_none(),
            "must reject {bad}"
        );
    }
}

#[test]
fn observed_quota_reset_scopes_to_profile_and_expires_automatically() {
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let svc = &fixture.service;
    let session = engine::for_tab(&tab).unwrap();
    let profile = "quota-reset-fixture";
    svc.mux
        .write()
        .get_tab_mut(&tab)
        .unwrap()
        .agent_runtime_config = Some(json!({ "launchProfile": profile }));
    let now = chrono::Utc::now().timestamp_millis().max(0) as u64;
    let resets_at = now + 300_000;
    let event = RuntimeEvent::ItemUpdated {
        thread_id: "own-thread".into(),
        item: ChatMsg {
            id: "turn-1:error".into(),
            seq: 1,
            kind: "system".into(),
            system_subtype: Some("api_error".into()),
            content: "Individual quota reached. Resets in 5m.".into(),
            tool_input: Some(json!({
                "retryable": false,
                "retryableKind": "rate_limited_until",
                "retryAtMs": resets_at
            })),
            ..Default::default()
        },
    };
    observe_rate_limit_reset(svc, &session, &event, now);
    let key = limits_source::balance_account_key(profile);
    let row = super::super::latest(svc, &session.provider, Some(&key)).unwrap();
    assert_eq!(row["usage_details"]["quota_reset"]["resets_at"], resets_at);
    assert_eq!(
        row["usage_details"]["quota_reset"]["reason"],
        "Individual quota reached. Resets in 5m."
    );
    let other_key = limits_source::balance_account_key("other-profile");
    assert!(
        super::super::latest(svc, &session.provider, Some(&other_key))
            .unwrap()
            .is_null()
    );
    assert!(active_quota_reset(&session.provider, Some(&key), resets_at).is_none());
    clear_quota_reset(&session.provider, Some(&key));
}

#[test]
fn statusline_quota_ingestion_requires_exact_tab_binding_and_persists_under_profile() {
    let (fixture, tab, _peer, _cleanup) = engine::test_support::runtime_fixture();
    let svc = &fixture.service;
    let profile = "sl-quota-fixture";
    let (provider, session_id, nonce) = {
        let mut mux = svc.mux.write();
        let entry = mux.get_tab_mut(&tab).unwrap();
        entry.agent_runtime_config = Some(json!({ "launchProfile": profile }));
        (
            entry.agent_type.clone().unwrap(),
            entry.agent_session_id.clone().unwrap(),
            entry.session_binding_nonce.clone().unwrap(),
        )
    };
    let now = 1_795_280_000_000_u64;
    let payload = json!({
        "session_id": session_id,
        "plan_tier": "Ultra",
        "quota": {
            "group-a": { "remaining_fraction": 0.6, "reset_in_seconds": 1800 }
        }
    });
    assert!(!ingest_statusline_quota_at(
        svc,
        &tab,
        "foreign-session",
        &nonce,
        &payload,
        now
    ));
    assert!(!ingest_statusline_quota_at(
        svc,
        &tab,
        &session_id,
        "foreign-nonce",
        &payload,
        now
    ));
    assert!(ingest_statusline_quota_at(
        svc,
        &tab,
        &session_id,
        &nonce,
        &payload,
        now
    ));
    let key = limits_source::balance_account_key(profile);
    let latest = super::super::latest(svc, &provider, Some(&key)).unwrap();
    assert_eq!(latest["parsed_ok"], true);
    assert_eq!(
        latest["usage_details"]["quota"]["title"],
        "Account quota (Ultra)"
    );
    assert_eq!(
        latest["usage_details"]["quota"]["limits"][0]["label"],
        "group-a"
    );
    assert_eq!(
        latest["usage_details"]["quota"]["limits"][0]["used_percent"],
        40.0
    );
}
