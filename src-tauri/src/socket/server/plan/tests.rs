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
        transfer_caller(&Some(json!({ "caller_tab_id": "source-manager" }))).unwrap(),
        "source-manager"
    );
    let missing = transfer_caller(&None).unwrap_err();
    assert_eq!(missing.code, -32602);
    assert!(missing.message.contains("caller_tab_id is required"));
}

#[test]
fn plan_create_uses_team_workspace_project_path() {
    let (mut mux, _) = crate::multiplexer::state::MultiplexerState::new();
    let workspace = mux.create_workspace("Team project".into(), Some("C:/team/workspace".into()));
    let tab_id = mux.create_tab(&workspace.id).unwrap();
    mux.get_tab_mut(&tab_id).unwrap().team_id = Some("team_1".into());

    let params = json!({ "caller_tab_id": tab_id.clone() });
    assert_eq!(plan_project_path(&params, &mux), "C:/team/workspace");
    assert_eq!(
        plan_project_path(
            &json!({ "caller_tab_id": tab_id, "project_path": "C:/override" }),
            &mux
        ),
        "C:/override"
    );
}
