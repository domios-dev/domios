use super::*;
use serde_json::json;

#[test]
fn rpc_rejects_unbounded_item_arrays_with_split_guidance() {
    let params = Some(json!({ "items": vec![json!({}); MAX_TAB_TODO_ITEMS + 1] }));
    let error = bounded_array::<Vec<TabTodoDraft>>(&params, "items").unwrap_err();
    assert_eq!(error.code, -32602);
    assert!(error.message.contains("propose a task split"));
}

#[test]
fn rpc_accepts_at_most_twelve_bounded_drafts() {
    let draft = json!({ "title": "A bounded todo" });
    let params = Some(json!({ "items": vec![draft; MAX_TAB_TODO_ITEMS] }));
    let items = bounded_array::<Vec<TabTodoDraft>>(&params, "items").expect("bounded list");
    assert_eq!(items.len(), MAX_TAB_TODO_ITEMS);
    assert_eq!(MAX_TAB_TODO_ITEMS, 12);
}

#[test]
fn rpc_state_accepts_blocked_and_skipped() {
    for (value, state) in [
        ("blocked", TabTodoState::Blocked),
        ("skipped", TabTodoState::Skipped),
    ] {
        assert_eq!(
            parse_state(&Some(json!({ "state": value }))).unwrap(),
            state
        );
    }
}

#[test]
fn rpc_write_accepts_an_empty_declaration_and_refuses_an_unbounded_one() {
    let empty = write_entries(&Some(json!({ "items": [] }))).expect("empty declaration");
    assert!(empty.is_empty());
    let entry = json!({ "content": "Step", "state": "in_progress" });
    let error = write_entries(&Some(
        json!({ "items": vec![entry; MAX_TAB_TODO_ITEMS + 1] }),
    ))
    .unwrap_err();
    assert!(error.message.contains("propose a task split"));
    assert!(write_entries(&Some(
        json!({ "items": [{ "content": "Step", "state": "open", "extra": 1 }] })
    ))
    .is_err());
}

fn proposal_fixture() -> (
    codeterm_service_core::service::test_fixture::ServiceFixture,
    String,
    String,
) {
    use codeterm_service_core::multiplexer::{state::MultiplexerState, types::AgentRole};
    let (mut mux, parent) = MultiplexerState::new();
    let workspace = mux.list_workspaces()[0].id.clone();
    let worker = mux.create_tab(&workspace).unwrap();
    mux.set_orchestrator_role(&parent).unwrap();
    mux.set_tab_role_kind(&worker, AgentRole::Worker, None, Some(&parent))
        .unwrap();
    let fixture = codeterm_service_core::service::test_fixture::ServiceFixture::new(mux);
    fixture.install_seeded_storage_desk();
    (fixture, parent, worker)
}

fn runtime_recipient(
    fixture: &codeterm_service_core::service::test_fixture::ServiceFixture,
    target: &str,
) {
    use codeterm_plugin::js_runtime::{JsProvider, PluginManifest, PluginVm};
    use codeterm_service_core::multiplexer::types::TabBacking;
    use std::sync::Arc;
    const PROVIDER: &str = "todo-runtime-fixture";
    let manifest = PluginManifest::from_json_str(
        &json!({
            "id": PROVIDER, "version": "1.0.0", "displayName": "Fixture",
            "capabilities": {
                "agentProvider": true,
                "chatBackend": {"spawnable": true, "sessionConfig": true, "agentTasks": true},
                "agentRuntime": {
                    "runtime": "stdio-jsonrpc",
                    "command": {"binary": "fixture-cli", "args": []},
                    "requestMethods": {"submitTurn": ["fixture/submit"]},
                    "notificationMethods": []
                }
            },
            "permissions": {"subprocess": {"allow": ["fixture-cli"]}}
        })
        .to_string(),
    )
    .unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(PROVIDER, "module.exports = {};".into()).unwrap()),
        )));
    let mut mux = fixture.service.mux.write();
    let tab = mux.get_tab_mut(target).unwrap();
    tab.agent_type = Some(PROVIDER.into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: PROVIDER.into(),
        session_id: "own-thread".into(),
    };
}

#[test]
fn rpc_proposal_review_runtime_notice_preserves_seeded_and_unseeded_checklists() {
    for seeded in [false, true] {
        for amend in [false, true] {
            let (fixture, parent, worker) = proposal_fixture();
            let _parent_deliveries =
                fixture.bind_recorded_agent_recipient(&parent, "parent-session", "parent-nonce");
            runtime_recipient(&fixture, &worker);
            let created = if seeded {
                dispatch(
                    "tab.todo.create",
                    &Some(json!({
                        "tab_id": worker, "caller_tab_id": parent,
                        "acceptance_criteria": ["Opaque criterion"]
                    })),
                    &std::sync::Arc::new(fixture.service.clone()),
                )
                .unwrap()
            } else {
                dispatch(
                    "tab.todo.write",
                    &Some(json!({
                        "tab_id": worker, "caller_tab_id": worker,
                        "items": [{"content": "Opaque step", "state": "open"}]
                    })),
                    &std::sync::Arc::new(fixture.service.clone()),
                )
                .unwrap()["checklist"]
                    .clone()
            };
            let service = std::sync::Arc::new(fixture.service.clone());
            dispatch(
                "tab.todo.submit",
                &Some(json!({
                    "tab_id": worker, "caller_tab_id": worker
                })),
                &service,
            )
            .unwrap();
            let mut params = json!({"tab_id": worker, "caller_tab_id": parent});
            if amend {
                params["items"] = json!([
                    {"id": created["items"][0]["id"], "state": "open"},
                    {"content": "Added review step", "state": "open"}
                ]);
            }
            let reviewed = dispatch(
                if amend {
                    "tab.todo.review_amend"
                } else {
                    "tab.todo.approve"
                },
                &Some(params),
                &service,
            )
            .unwrap();
            assert_eq!(
                reviewed["proposal"]["state"],
                if amend { "amended" } else { "approved" }
            );
            assert_eq!(reviewed["items"][0]["locked"], seeded);
            assert_eq!(
                reviewed["items"].as_array().unwrap().len(),
                if amend { 2 } else { 1 }
            );
            let persisted = dispatch(
                "tab.todo.list",
                &Some(json!({
                    "tab_id": worker, "caller_tab_id": worker
                })),
                &service,
            )
            .unwrap();
            assert_eq!(persisted, reviewed);
        }
    }
}

#[test]
fn rpc_proposal_submit_runtime_notice_persists_shared_checklist() {
    let (fixture, parent, worker) = proposal_fixture();
    runtime_recipient(&fixture, &parent);
    let service = std::sync::Arc::new(fixture.service.clone());
    dispatch(
        "tab.todo.create",
        &Some(json!({
            "tab_id": worker, "caller_tab_id": parent,
            "acceptance_criteria": ["Opaque criterion"]
        })),
        &service,
    )
    .unwrap();
    let shared = dispatch(
        "tab.todo.submit",
        &Some(json!({
            "tab_id": worker, "caller_tab_id": worker
        })),
        &service,
    )
    .unwrap();
    assert_eq!(shared["proposal"]["state"], "proposed");
    let persisted = dispatch(
        "tab.todo.list",
        &Some(json!({
            "tab_id": worker, "caller_tab_id": worker
        })),
        &service,
    )
    .unwrap();
    assert_eq!(persisted, shared);
}
