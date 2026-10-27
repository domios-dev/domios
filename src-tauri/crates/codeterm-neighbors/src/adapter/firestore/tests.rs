use super::*;

#[test]
fn firestore_requests_keep_account_tokens_sensitive_and_reject_invalid_metadata() {
    let database = "projects/fixture-project/databases/(default)";
    let request = authenticated_request("opaque-token", database, ()).unwrap();
    let authorization = request.metadata().get("authorization").unwrap();
    assert_eq!(authorization.to_str().unwrap(), "Bearer opaque-token");
    assert!(authorization.is_sensitive());
    assert_eq!(
        request
            .metadata()
            .get("google-cloud-resource-prefix")
            .unwrap()
            .to_str()
            .unwrap(),
        database
    );
    assert!(authenticated_request("invalid\ntoken", database, ()).is_err());
}

#[test]
fn target_current_signals_ready_even_for_an_empty_inbox() {
    let mut current = fs::TargetChange {
        target_change_type: fs::target_change::TargetChangeType::Current as i32,
        target_ids: vec![1],
        ..Default::default()
    };
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(checkpoint.ready);
    assert!(!checkpoint.flush);
    current.read_time = Some(prost_types::Timestamp {
        seconds: 10,
        nanos: 0,
    });
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(checkpoint.ready && checkpoint.flush);
    current.target_ids = vec![2];
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(!checkpoint.ready && !checkpoint.flush);
    current.target_ids = vec![1];
    current.target_change_type = fs::target_change::TargetChangeType::NoChange as i32;
    let checkpoint = target_checkpoint(&current).unwrap();
    assert!(!checkpoint.ready && checkpoint.flush);
}
