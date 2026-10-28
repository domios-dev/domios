use super::*;
mod size_boundary;

#[test]
fn grpc_failure_keeps_code_operation_and_only_safe_diagnostic_fields() {
    struct Calls(std::sync::Mutex<Vec<(String, String, Option<i32>, String)>>);
    #[async_trait]
    impl AuthenticatedCall for Calls {
        fn firestore_failure(
            &self,
            operation: &'static str,
            target: &str,
            code: Option<i32>,
            kind: &str,
        ) {
            self.0
                .lock()
                .unwrap()
                .push((operation.into(), target.into(), code, kind.into()));
        }
        async fn invoke(&self, _: &str, _: serde_json::Value) -> Result<serde_json::Value> {
            unreachable!()
        }
        async fn id_token(&self) -> Result<Zeroizing<String>> {
            unreachable!()
        }
    }
    let calls = Calls(Default::default());
    let error = rpc_failure(
        &calls,
        "inbox.run_query",
        "projects/fixture/documents/friendInbox/owner",
        tonic::Status::permission_denied("opaque-sensitive-server-detail"),
    );
    assert_eq!(
        error,
        Error::Firestore {
            code: 7,
            operation: "inbox.run_query"
        }
    );
    assert_eq!(
        error.to_string(),
        "Firestore inbox.run_query failed (gRPC code 7)"
    );
    let diagnostic = calls.0.lock().unwrap();
    assert_eq!(diagnostic[0].2, Some(7));
    assert_eq!(diagnostic[0].3, "PermissionDenied");
    assert!(!format!("{diagnostic:?}").contains("opaque-sensitive-server-detail"));
}

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
            .get("x-goog-request-params")
            .unwrap()
            .to_str()
            .unwrap(),
        format!("database={database}")
    );
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
