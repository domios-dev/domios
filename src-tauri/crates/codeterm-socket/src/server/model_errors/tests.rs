use super::*;

#[test]
fn spawn_profile_error_keeps_typed_rpc_data() {
    let refusal = codeterm_service_core::service::spawn_params::LaunchProfileRefusal {
        code: "unknown_launch_profile".into(),
        provider: "fixture".into(),
        requested_profile: Some("nope".into()),
        declared_profile_ids: vec!["selected".into()],
        message: "unknown_launch_profile: fixture/nope; declared profile ids: [selected]".into(),
    };
    let error = launch_profile_rpc_error(refusal);
    assert_eq!(error.code, -32602);
    let data = error.data.unwrap();
    assert_eq!(data["code"], "unknown_launch_profile");
    assert_eq!(
        data["declared_profile_ids"],
        serde_json::json!(["selected"])
    );
}
