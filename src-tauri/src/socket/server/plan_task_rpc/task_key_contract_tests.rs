use super::dispatch;
use crate::multiplexer::state::MultiplexerState;
use crate::plan::{OrchestratorRef, Plan};
use crate::service::plan::task_key_port_impl::MuxTaskKeyPort;
use crate::service::test_fixture::ServiceFixture;
use serde_json::{json, Value};
use std::sync::Arc;

/// A live owner in the root "Main" workspace, a Team-bound plan, and the
/// production mux-backed key port.
fn keyed_fixture() -> (ServiceFixture, String, String) {
    let (mut mux, _) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let owner = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&owner).unwrap();
    let team_id = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
    {
        let tab = mux.get_tab_mut(&owner).unwrap();
        tab.agent_session_id = Some("owner-session".into());
        tab.session_binding_nonce = Some("owner-nonce".into());
        tab.reported_session_binding_nonce = Some("owner-nonce".into());
    }
    let fixture = ServiceFixture::new(mux);
    fixture
        .service
        .plan_store
        .bind_task_key_port(Arc::new(MuxTaskKeyPort::new(
            fixture.service.mux.clone(),
            fixture.service.app_handle.clone(),
        )));
    let mut plan = Plan::new(
        "Task keys".into(),
        fixture.temp.path().display().to_string(),
        OrchestratorRef {
            tab_id: Some(owner.clone()),
            session_id: Some("owner-session".into()),
            title: None,
        },
    );
    plan.team_id = Some(team_id);
    fixture.service.plan_store.save(&mut plan, 1).unwrap();
    (fixture, plan.id, owner)
}

fn owner_call(owner: &str, fields: Value) -> Option<Value> {
    let mut params = json!({
        "caller_tab_id": owner,
        "session_id": "owner-session",
        "session_binding_nonce": "owner-nonce",
    });
    for (field, value) in fields.as_object().unwrap() {
        params[field] = value.clone();
    }
    Some(params)
}

#[cfg_attr(not(windows), ignore = "requires a desktop runtime")]
#[test]
fn task_key_contract_socket_add_returns_key_and_reads_accept_id_or_key() {
    let (fixture, plan_id, owner) = keyed_fixture();
    let service = Arc::new(fixture.service.clone());
    let app = &fixture.service.app_handle;

    let created = dispatch(
        "plan.task.add",
        &owner_call(&owner, json!({ "plan_id": plan_id, "name": "Keyed task" })),
        app,
        &service,
        2,
    )
    .unwrap_or_else(|error| panic!("add failed: {} {:?}", error.message, error.data));
    let task_id = created["task_id"].as_str().unwrap().to_string();
    let key = created["key"].as_str().unwrap().to_string();
    assert!(task_id.starts_with("task_"), "{task_id}");
    assert_eq!(key, "MAI-1");
    let workspace = fixture.service.mux.read().list_workspaces()[0].id.clone();
    assert_eq!(
        fixture
            .service
            .mux
            .read()
            .workspace_task_keys(&workspace)
            .map(|keys| (keys.prefix.clone(), keys.next)),
        Some((Some("MAI".to_string()), 2))
    );

    for reference in [task_id.clone(), key.clone(), key.to_ascii_lowercase()] {
        for plan_scoped in [true, false] {
            let mut fields = json!({ "task_id": reference });
            if plan_scoped {
                fields["plan_id"] = json!(plan_id);
            }
            let detail = dispatch("task.show", &Some(fields), app, &service, 3)
                .unwrap_or_else(|error| panic!("show {reference}: {}", error.message));
            assert_eq!(detail["task"]["id"], task_id, "{reference}");
            assert_eq!(detail["task"]["key"], key, "{reference}");
            assert_eq!(detail["display_key"], key, "{reference}");
        }
    }

    let second = dispatch(
        "plan.task.add",
        &owner_call(&owner, json!({ "plan_id": plan_id, "name": "Second task" })),
        app,
        &service,
        4,
    )
    .unwrap_or_else(|error| panic!("second add failed: {}", error.message));
    assert_eq!(second["key"], "MAI-2");
    assert_eq!(
        fixture
            .service
            .plan_store
            .task_display_key(&plan_id, &task_id)
            .as_deref(),
        Some(key.as_str())
    );
}
