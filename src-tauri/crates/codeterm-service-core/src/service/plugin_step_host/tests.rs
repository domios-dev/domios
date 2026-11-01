use super::*;

#[test]
fn step_run_cwd_must_match_host_stamped_authored_params() {
    let directory = tempfile::tempdir().unwrap();
    let authored = directory.path().to_string_lossy().to_string();
    let mut context = StepInputContext {
        plugin:"fixture".into(),submitting:true,deadline:std::time::Instant::now(),cancellation:Default::default(),surfaces:Default::default(),
        request:serde_json::from_value(serde_json::json!({"context":{"scope":{"workspace":"w","team":null,"run":"r"},"owner":"owner","node":"n","attempt":"a","operation":"op","nodeLabel":"n","inputLineage":[]},"nodeType":"fixture.run_prompt","revision":1,"params":{"cwd":authored},"inputs":{}})).unwrap(),
    };
    assert!(authored_cwd(&context, &authored).is_ok());
    assert!(authored_cwd(&context, "/foreign").is_err());
    context.request.params = serde_json::json!({});
    assert!(authored_cwd(&context, &authored).is_err());
}

#[test]
fn step_run_start_rejects_keep_option() {
    let options = serde_json::json!({"prompt":"opaque","cwd":"/work","timeoutMs":1000,"keep":true});
    assert!(serde_json::from_value::<StartOptions>(options).is_err());
}

#[test]
fn step_run_plugin_cannot_override_host_access_policy() {
    for field in [
        "skipPermissions",
        "skip_permissions",
        "fullAccess",
        "permissionBypassAuthorization",
    ] {
        let mut options = serde_json::json!({"prompt":"opaque","cwd":"/work","timeoutMs":1000});
        options[field] = serde_json::json!(true);
        assert!(serde_json::from_value::<StartOptions>(options).is_err());
    }
}
