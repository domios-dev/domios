use super::*;
use serde_json::json;

fn loaded(current: Option<&str>, advertised: &[&str]) -> RuntimeSettings {
    serde_json::from_value(json!({
        "mode": current,
        "modes": advertised.iter().map(|id| json!({"id": id, "name": id})).collect::<Vec<_>>(),
    }))
    .unwrap()
}

/// A session exposing its mode both ways; only the option moves when `set_config_option` is used.
fn through_option(session_mode: &str, option_current: &str) -> RuntimeSettings {
    serde_json::from_value(json!({
        "mode": session_mode,
        "modes": [
            {"id": "opaque-default", "name": "Default"},
            {"id": "opaque-approve", "name": "Approve"},
        ],
        "configOptions": [{
            "id": "opaque-mode",
            "name": "Mode",
            "category": "mode",
            "currentValue": option_current,
            "options": [
                {"value": "opaque-default", "name": "Default"},
                {"value": "opaque-approve", "name": "Approve"},
            ],
        }],
    }))
    .unwrap()
}

#[test]
fn a_mode_reached_through_its_config_option_is_no_failure_while_the_session_mode_lags() {
    assert_eq!(
        missing_full_access(
            &json!({"fullAccessMode": "opaque-approve"}),
            &through_option("opaque-default", "opaque-approve"),
        ),
        None
    );
}

#[test]
fn a_mode_option_off_the_declared_mode_is_named_even_if_the_session_mode_claims_it() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-approve"}),
        &through_option("opaque-approve", "opaque-default"),
    )
    .unwrap();
    assert!(failure.starts_with(FULL_ACCESS_UNAVAILABLE), "{failure}");
    assert!(failure.contains("refused"), "{failure}");
    assert!(failure.contains("running in opaque-default"), "{failure}");
}

#[test]
fn a_mode_only_the_option_offers_is_refused_rather_than_unadvertised() {
    let settings = RuntimeSettings {
        mode: None,
        modes: Some(Vec::new()),
        ..through_option("opaque-default", "opaque-default")
    };
    let failure =
        missing_full_access(&json!({"fullAccessMode": "opaque-approve"}), &settings).unwrap();
    assert!(failure.contains("refused"), "{failure}");
    assert_eq!(
        missing_full_access(
            &json!({"fullAccessMode": "opaque-approve"}),
            &RuntimeSettings {
                modes: Some(Vec::new()),
                ..through_option("opaque-default", "opaque-approve")
            },
        ),
        None
    );
}

#[test]
fn a_mode_neither_the_option_nor_the_session_offers_is_named_with_what_they_do() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-stale"}),
        &through_option("opaque-default", "opaque-default"),
    )
    .unwrap();
    assert!(failure.contains("not advertised"), "{failure}");
    assert!(
        failure.contains("opaque-default, opaque-approve"),
        "{failure}"
    );
}

#[test]
fn declared_mode_absent_from_the_advertised_modes_is_named_with_what_the_agent_offers() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-stale"}),
        &loaded(
            Some("opaque-default"),
            &["opaque-default", "opaque-approve"],
        ),
    )
    .unwrap();
    assert!(failure.starts_with(FULL_ACCESS_UNAVAILABLE), "{failure}");
    for part in ["opaque-stale", "opaque-default, opaque-approve"] {
        assert!(failure.contains(part), "{failure}");
    }
}

#[test]
fn an_agent_with_no_modes_cannot_silently_keep_a_declared_mode() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-approve"}),
        &loaded(None, &[]),
    )
    .unwrap();
    assert!(failure.contains("no modes"), "{failure}");
}

#[test]
fn a_refused_advertised_mode_is_named_as_refused() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-approve"}),
        &loaded(
            Some("opaque-default"),
            &["opaque-default", "opaque-approve"],
        ),
    )
    .unwrap();
    assert!(failure.contains("refused"), "{failure}");
    assert!(failure.contains("opaque-default"), "{failure}");
}

#[test]
fn an_owner_choice_of_the_declared_mode_that_the_session_lost_is_still_named() {
    let failure = missing_full_access(
        &json!({"fullAccessMode": "opaque-approve", "sessionMode": "opaque-approve"}),
        &loaded(Some("opaque-default"), &["opaque-default"]),
    )
    .unwrap();
    assert!(failure.contains("not advertised"), "{failure}");
}

#[test]
fn reaching_the_mode_or_declaring_none_or_an_owner_choice_is_not_a_failure() {
    let settings = loaded(
        Some("opaque-default"),
        &["opaque-default", "opaque-approve"],
    );
    assert_eq!(missing_full_access(&json!({}), &settings), None);
    assert_eq!(
        missing_full_access(&serde_json::Value::Null, &settings),
        None
    );
    assert_eq!(
        missing_full_access(
            &json!({"fullAccessMode": "opaque-approve", "sessionMode": "opaque-default"}),
            &settings
        ),
        None
    );
    assert_eq!(
        missing_full_access(
            &json!({"fullAccessMode": "opaque-approve"}),
            &loaded(Some("opaque-approve"), &["opaque-approve"])
        ),
        None
    );
}
