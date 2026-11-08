use super::*;
use serde_json::json;

fn loaded(current: Option<&str>, advertised: &[&str]) -> RuntimeSettings {
    serde_json::from_value(json!({
        "mode": current,
        "modes": advertised.iter().map(|id| json!({"id": id, "name": id})).collect::<Vec<_>>(),
    }))
    .unwrap()
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
