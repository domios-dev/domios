use super::{admit_key, ApiErrorKind, ProviderTurnIncident, SessionObservations};
use serde_json::{json, Value};

fn class_of(record: &Value) -> ApiErrorKind {
    let error = record.get("error").or(record
        .get("payload")
        .and_then(|payload| payload.get("error")));
    let scope = record.get("payload").unwrap_or(record);
    ApiErrorKind::from_error(error.or(Some(scope)), scope)
}

#[test]
fn recorded_fixtures_map_onto_one_declared_class() {
    let fixtures: Value = serde_json::from_str(include_str!(
        "../../../../../../plugins/provider-shared/access-errors.json"
    ))
    .unwrap();
    let mut checked = 0;
    for fixture in fixtures.as_array().unwrap() {
        if fixture["absent"] == true || fixture["clears"] == true {
            continue;
        }
        let expected: ApiErrorKind = serde_json::from_value(fixture["expected"].clone()).unwrap();
        if fixture["claude"]["isApiErrorMessage"] == true {
            assert_eq!(class_of(&fixture["claude"]), expected);
            checked += 1;
        }
        if fixture["codex"]["payload"]["type"] == "error" {
            assert_eq!(class_of(&fixture["codex"]), expected);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn codex_runtime_codes_use_the_declared_classes() {
    for (error, class) in [
        (
            json!({"codexErrorInfo":"usageLimitExceeded"}),
            ApiErrorKind::UsageLimit,
        ),
        (
            json!({"codexErrorInfo":"rateLimitExceeded"}),
            ApiErrorKind::RateLimit,
        ),
        (
            json!({"codexErrorInfo":"serverOverloaded"}),
            ApiErrorKind::ServerOverloaded,
        ),
        (
            json!({"codexErrorInfo":"internalServerError"}),
            ApiErrorKind::ServerOverloaded,
        ),
        (
            json!({"status":503,"codexErrorInfo":{"httpConnectionFailed":{"httpStatusCode":503}}}),
            ApiErrorKind::ServerOverloaded,
        ),
        (json!("token_expired"), ApiErrorKind::AuthExpiry),
        (json!({"status":429}), ApiErrorKind::RateLimit),
        (
            json!({"codexErrorInfo":{"usageLimitExceeded":null},"status":403}),
            ApiErrorKind::Authentication,
        ),
    ] {
        assert_eq!(ApiErrorKind::from_error(Some(&error), &error), class);
        assert_eq!(serde_json::to_value(class).unwrap(), class.as_str());
    }
}

#[test]
fn one_failed_turn_admits_one_incident_and_the_same_key_adds_none() {
    let observation = SessionObservations {
        api_error: Some(ApiErrorKind::RateLimit),
        retry_after_secs: Some(12),
        turn_id: Some("turn-429".into()),
        ..Default::default()
    };
    let incident = ProviderTurnIncident::from_observation(
        "worker",
        Some("task"),
        "claude",
        &observation,
        None,
    )
    .unwrap();
    assert_eq!(incident.worker, "worker");
    assert_eq!(incident.task.as_deref(), Some("task"));
    assert_eq!(incident.provider, "claude");
    assert_eq!(incident.class, ApiErrorKind::RateLimit);
    assert_eq!(incident.retry_after, Some(12));
    assert!(incident.turn_stopped);
    assert!(!incident.pauses_for_exhaustion);
    assert_eq!(
        incident.idempotency_key,
        "provider-error:worker:turn-429:rate_limit"
    );
    let mut seen = Vec::new();
    assert!(admit_key(&mut seen, &incident.idempotency_key));
    assert!(!admit_key(&mut seen, &incident.idempotency_key));
    let again = ProviderTurnIncident::from_observation(
        "worker",
        Some("task"),
        "claude",
        &observation,
        None,
    )
    .unwrap();
    assert_eq!(again.idempotency_key, incident.idempotency_key);
    assert!(!admit_key(&mut seen, &again.idempotency_key));
}

#[test]
fn usage_limit_pauses_for_exhaustion_and_auth_loss_stays_on_its_own_path() {
    let usage = ProviderTurnIncident::from_observation(
        "worker",
        Some("task"),
        "codex",
        &SessionObservations {
            api_error: Some(ApiErrorKind::UsageLimit),
            turn_id: Some("turn-usage".into()),
            ..Default::default()
        },
        None,
    )
    .unwrap();
    assert!(usage.pauses_for_exhaustion);
    assert!(usage.turn_stopped);
    assert!(ProviderTurnIncident::from_observation(
        "worker",
        Some("task"),
        "codex",
        &SessionObservations {
            api_error: Some(ApiErrorKind::Authentication),
            turn_id: Some("turn-auth".into()),
            ..Default::default()
        },
        None,
    )
    .is_none());
    assert!(ProviderTurnIncident::from_observation(
        "worker",
        Some("task"),
        "codex",
        &SessionObservations {
            api_error: Some(ApiErrorKind::RateLimit),
            turn_id: Some("turn-retry".into()),
            retrying: true,
            ..Default::default()
        },
        None,
    )
    .is_none());
}

#[test]
fn retry_after_and_turn_id_come_from_the_record() {
    let error = json!({"status":429,"retryAfter":"30"});
    let record = json!({"uuid":"turn-429","error":error});
    assert_eq!(ApiErrorKind::retry_after(Some(&error), &record), Some(30));
    assert_eq!(ApiErrorKind::turn_id(&record).as_deref(), Some("turn-429"));
    assert!(!ApiErrorKind::retrying(Some(&error), &record));
    assert!(ApiErrorKind::retrying(
        Some(&json!({"willRetry":true})),
        &json!({})
    ));
}
