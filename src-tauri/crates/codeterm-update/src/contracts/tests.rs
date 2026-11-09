use super::*;
use serde_json::json;

#[test]
fn install_response_omits_unused_fields() {
    assert_eq!(
        serde_json::to_value(InstallResponse::default()).unwrap(),
        json!({"status":"up_to_date"})
    );
    let response = InstallResponse {
        status: InstallStatus::Command,
        command: Some("install".into()),
        message: Some("Run: install".into()),
        ..Default::default()
    };
    assert_eq!(
        serde_json::to_value(response).unwrap(),
        json!({"status":"command","command":"install","message":"Run: install"})
    );
}
