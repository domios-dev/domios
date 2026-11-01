use super::*;
use codeterm_service_core::multiplexer::state::MultiplexerState;
use codeterm_service_core::plan::{OrchestratorRef, Plan, TaskStatus};
use codeterm_service_core::service::plan::NewTask;
use codeterm_service_core::service::test_fixture::ServiceFixture;
use std::sync::Arc;

#[test]
fn task_pause_transport_removed() {
    assert!(!handles("plan.task.pause"));
    assert!(!handles("plan.task.resume"));
    assert!(handles("plan.task.close"));
}

#[test]
fn owner_close_and_record_persist_direct_completion_for_each_open_status() {
    for status in [
        TaskStatus::Pending,
        TaskStatus::Blocked,
        TaskStatus::InProgress,
    ] {
        for batch in [false, true] {
            let (mut mux, _) = MultiplexerState::new();
            let workspace = mux.list_workspaces()[0].id.clone();
            let owner = mux.create_tab(&workspace).unwrap();
            mux.set_orchestrator_role(&owner).unwrap();
            let team = mux.resolve_or_mint_team_for_parent(&owner).unwrap().team_id;
            let tab = mux.get_tab_mut(&owner).unwrap();
            tab.agent_session_id = Some("session".into());
            tab.session_binding_nonce = Some("nonce".into());
            tab.reported_session_binding_nonce = Some("nonce".into());
            let fixture = ServiceFixture::new(mux);
            let svc = Arc::new(fixture.service.clone());
            let mut plan = Plan::new(
                "opaque-plan".into(),
                fixture.temp.path().display().to_string(),
                OrchestratorRef::from_tab(&owner),
            );
            plan.team_id = Some(team);
            svc.plan_store.save(&mut plan, 1).unwrap();
            let task = svc
                .plan_store
                .add_task(
                    &plan.id,
                    NewTask {
                        name: "opaque-task".into(),
                        ..Default::default()
                    },
                    2,
                )
                .unwrap();
            svc.plan_store
                .with_plan(&plan.id, 3, |plan| {
                    plan.tasks[0].status = status.clone();
                    Ok(())
                })
                .unwrap();
            let mut params = json!({
                "plan_id":plan.id, "task_id":task,
                "caller_tab_id":owner, "session_id":"session", "session_binding_nonce":"nonce"
            });
            for (note, succeeds) in [(" \r\n", false), ("opaque-result-72", true)] {
                params["result"] = json!(note);
                params["ops"] = json!([
                    {"op":"set_task_status", "client_id":"task", "task":task, "status":"done"},
                    {"op":"add_milestone", "task":"$task", "kind":"status_update", "text":note}
                ]);
                let request = Some(params.clone());
                let result = if batch {
                    super::super::plan::dispatch("plan.batch", &request, &svc)
                } else {
                    dispatch("plan.task.close", &request, &svc)
                };
                assert_eq!(
                    result.is_ok(),
                    succeeds,
                    "{status:?}, batch={batch}: {result:?}"
                );
                let persisted = svc.plan_store.show_task(&task).unwrap().task;
                assert_eq!(
                    persisted.status,
                    if succeeds {
                        TaskStatus::Done
                    } else {
                        status.clone()
                    }
                );
                if succeeds {
                    assert_eq!(persisted.milestones.last().unwrap().text, note);
                } else {
                    assert!(persisted.milestones.is_empty());
                }
            }
        }
    }
}
