use std::collections::HashMap;
use std::sync::Arc;

use serde_json::json;

use crate::lock_hold::TimedRwLock;
use crate::multiplexer::state::MultiplexerState;
use crate::service::plan::ports::TaskKeyScope;
use crate::service::plan::task_key_port_impl::issue_from_mux;
use crate::session::store::{SessionSnapshot, WorkspaceSnapshot};

fn scope(project_path: &str, team_id: Option<&str>) -> TaskKeyScope {
    TaskKeyScope {
        plan_id: "pl_keys".into(),
        team_id: team_id.map(str::to_string),
        project_path: project_path.into(),
        canonical_project_path: project_path.into(),
    }
}

#[test]
fn task_key_contract_colliding_workspaces_get_distinct_immutable_prefixes() {
    let (mut mux, _) = MultiplexerState::new();
    let term = mux
        .create_workspace("CodeTerm".into(), Some("/work/codeterm".into()))
        .id;
    let codex = mux
        .create_workspace("Codex".into(), Some("/work/codex".into()))
        .id;
    let none = HashMap::new();

    assert_eq!(mux.issue_task_keys(&term, 1, &none), vec!["COD-1"]);
    assert_eq!(mux.issue_task_keys(&codex, 2, &none), vec!["CO-1", "CO-2"]);
    mux.rename_workspace(&term, "Zeta").unwrap();
    assert_eq!(mux.issue_task_keys(&term, 1, &none), vec!["COD-2"]);

    let persisted = serde_json::to_value(mux.to_session_snapshot()).unwrap();
    let snapshot: SessionSnapshot = serde_json::from_value(persisted).unwrap();
    let mut restored = MultiplexerState::from_session_snapshot(&snapshot);
    assert_eq!(restored.issue_task_keys(&codex, 1, &none), vec!["CO-3"]);
    assert_eq!(restored.issue_task_keys(&term, 1, &none), vec!["COD-3"]);
    assert_eq!(
        restored
            .workspace_task_keys(&term)
            .and_then(|keys| keys.prefix.clone())
            .as_deref(),
        Some("COD")
    );
}

#[test]
fn task_key_contract_legacy_workspace_snapshot_loads_without_key_state() {
    let legacy: WorkspaceSnapshot = serde_json::from_value(json!({
        "id": "ws_old",
        "name": "Old",
        "tabs": [],
        "active_tab_id": null
    }))
    .unwrap();
    assert!(legacy.task_keys.is_unissued());
    assert!(serde_json::to_value(&legacy)
        .unwrap()
        .get("task_keys")
        .is_none());
}

#[test]
fn task_key_contract_plan_scope_resolves_team_home_then_project_folder() {
    let (mut mux, _) = MultiplexerState::new();
    let home = mux
        .create_workspace("Home".into(), Some("/work/home".into()))
        .id;
    let project = mux
        .create_workspace("Project".into(), Some("/work/project".into()))
        .id;
    let anchor = mux.create_tab(&home).unwrap();
    mux.set_orchestrator_role(&anchor).unwrap();
    let team_id = mux
        .resolve_or_mint_team_for_parent(&anchor)
        .unwrap()
        .team_id;

    assert_eq!(
        mux.task_key_workspace(None, &["/work/project"]).as_deref(),
        Some(project.as_str())
    );
    assert_eq!(
        mux.task_key_workspace(Some(&team_id), &["/work/project"])
            .as_deref(),
        Some(home.as_str())
    );
    assert_eq!(mux.task_key_workspace(None, &["", "/work/nowhere"]), None);

    let mux = Arc::new(TimedRwLock::new("mux", mux));
    assert_eq!(
        issue_from_mux(&mux, &scope("/work/project", None), 1, &HashMap::new()),
        vec!["PRO-1"]
    );
    assert_eq!(
        issue_from_mux(
            &mux,
            &scope("/work/nowhere", Some(&team_id)),
            1,
            &HashMap::new()
        ),
        vec!["HOM-1"]
    );
    assert!(issue_from_mux(&mux, &scope("/work/nowhere", None), 1, &HashMap::new()).is_empty());
    let stored_above = HashMap::from([("PRO".to_string(), 41)]);
    assert_eq!(
        issue_from_mux(&mux, &scope("/work/project", None), 1, &stored_above),
        vec!["PRO-42"]
    );
}
