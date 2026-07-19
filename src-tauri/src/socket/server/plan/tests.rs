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
fn transfer_caller_requires_agent_param() {
    assert_eq!(
        transfer_caller(&Some(json!({ "caller_pane_id": "source-manager" }))).unwrap(),
        "source-manager"
    );
    let missing = transfer_caller(&None).unwrap_err();
    assert_eq!(missing.code, -32602);
    assert!(missing.message.contains("caller_pane_id is required"));
}
