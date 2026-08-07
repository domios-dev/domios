//! Workspace resolution and reuse-notice output contracts.
//!
//! Split out of the shared test root: these assert `cmd::local_io` behaviour,
//! not subcommand parse/routing shape, and the root is at the 1200-line limit.

use crate::cmd;

#[test]
fn workspace_reuse_notice_mentions_actual_and_requested_names() {
    let value = serde_json::json!({
        "id": "abcd1234",
        "name": "Real Workspace",
        "existing": true
    });
    assert_eq!(
            cmd::local_io::workspace_reuse_notice(&value, "Requested Workspace").as_deref(),
            Some("reused existing workspace abcd1234 'Real Workspace' (same cwd); requested name 'Requested Workspace' ignored")
        );
}

#[test]
fn workspace_reuse_notice_absent_for_new_or_unmarked_workspace() {
    let created = serde_json::json!({
        "id": "abcd1234",
        "name": "Requested Workspace",
        "existing": false
    });
    let unmarked = serde_json::json!({
        "id": "abcd1234",
        "name": "Requested Workspace"
    });
    assert!(cmd::local_io::workspace_reuse_notice(&created, "Requested Workspace").is_none());
    assert!(cmd::local_io::workspace_reuse_notice(&unmarked, "Requested Workspace").is_none());
}

#[test]
fn workspace_reuse_human_output_goes_to_stderr_only() {
    let value = serde_json::json!({
        "id": "abcd1234",
        "name": "Real Workspace",
        "existing": true
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert!(cmd::local_io::write_workspace_create_reuse_output(
        &value,
        "Requested Workspace",
        false,
        &mut stdout,
        &mut stderr
    )
    .unwrap());

    assert!(stdout.is_empty());
    assert_eq!(
            String::from_utf8(stderr).unwrap(),
            "reused existing workspace abcd1234 'Real Workspace' (same cwd); requested name 'Requested Workspace' ignored\n"
        );
}

#[test]
fn workspace_reuse_json_output_stays_on_stdout_only() {
    let value = serde_json::json!({
        "existing": true,
        "id": "abcd1234",
        "name": "Real Workspace"
    });
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();

    assert!(cmd::local_io::write_workspace_create_reuse_output(
        &value,
        "Requested Workspace",
        true,
        &mut stdout,
        &mut stderr
    )
    .unwrap());

    assert!(stderr.is_empty());
    assert_eq!(
        String::from_utf8(stdout).unwrap(),
        format!("{}\n", serde_json::to_string_pretty(&value).unwrap())
    );
}

#[test]
fn workspace_current_prefers_the_calling_tabs_workspace() {
    let workspaces = serde_json::json!([
        {"id":"focused","is_active":true,"tab_ids":["other"]},
        {"id":"caller","is_active":false,"tab_ids":["self_tab"]}
    ]);
    assert_eq!(
        cmd::local_io::workspace_current_id(workspaces.as_array().unwrap(), Some("self_tab")),
        Some("caller")
    );
}

#[test]
fn workspace_current_falls_back_to_global_focus_outside_a_tab() {
    let workspaces = serde_json::json!([
        {"id":"first","is_active":false},
        {"id":"focused","is_active":true}
    ]);
    assert_eq!(
        cmd::local_io::workspace_current_id(workspaces.as_array().unwrap(), None),
        Some("focused")
    );
}

#[test]
fn workspace_current_returns_nothing_without_caller_or_focus() {
    let workspaces = serde_json::json!([
        {"id":"first","is_active":false},
        {"id":"second","is_active":false}
    ]);

    assert_eq!(
        cmd::local_io::workspace_current_id(workspaces.as_array().unwrap(), None),
        None
    );
}

#[test]
fn workspace_current_resolution_reports_caller_source() {
    let workspaces = serde_json::json!([
        {"id":"focused","is_active":true,"tab_ids":[]},
        {"id":"caller","is_active":false,"tab_ids":["self_tab"]}
    ]);

    let resolved = cmd::local_io::workspace_current_resolution(
        workspaces.as_array().unwrap(),
        cmd::local_io::WorkspaceIdentity::caller_tab(Some("self_tab")),
    )
    .unwrap();

    assert_eq!(resolved.id, "caller");
    assert_eq!(resolved.source.as_str(), "caller-tab");
}

#[test]
fn workspace_current_resolution_reports_focus_fallback_source() {
    let workspaces = serde_json::json!([
        {"id":"inactive","is_active":false,"tab_ids":[]},
        {"id":"focused","is_active":true,"tab_ids":[]}
    ]);

    let resolved = cmd::local_io::workspace_current_resolution(
        workspaces.as_array().unwrap(),
        cmd::local_io::WorkspaceIdentity::caller_tab(None),
    )
    .unwrap();

    assert_eq!(resolved.id, "focused");
    assert_eq!(resolved.source.as_str(), "focus-fallback");
}
