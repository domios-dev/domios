use super::*;
use crate::socket::server::plan_fields::optional_string_field;

#[test]
fn optional_string_field_preserves_missing_vs_null() {
    let params = json!({
        "model": "opus",
        "why": null
    });

    assert_eq!(optional_string_field(Some(&params), "missing"), None);
    assert_eq!(optional_string_field(Some(&params), "why"), Some(None));
    assert_eq!(
        optional_string_field(Some(&params), "model"),
        Some(Some("opus"))
    );
}

#[test]
fn transfer_caller_distinguishes_agent_param_from_trusted_direct_user() {
    assert_eq!(
        transfer_caller(&Some(json!({ "caller_pane_id": "source-manager" }))),
        crate::service::TeamMutationCaller::AgentPane("source-manager".into())
    );
    assert_eq!(
        transfer_caller(&None),
        crate::service::TeamMutationCaller::TrustedDirectUser
    );
}
