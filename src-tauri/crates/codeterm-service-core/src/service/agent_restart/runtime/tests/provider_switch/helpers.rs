use super::*;

pub(super) fn complete_switch_task(fixture: &RuntimeFixture, task_id: &str) {
    let detail = fixture
        .service
        .plan_store
        .inspect_task_for_test(task_id)
        .unwrap();
    fixture
        .service
        .plan_store
        .edit_plan_fixture(&detail.plan_id, 5, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .unwrap();
            task.status = crate::plan::TaskStatus::Done;
            task.assigned_tab_id = None;
            task.lane.tab_id = None;
            task.execution_attempts[0].closed_at = Some(5);
            Ok(())
        })
        .unwrap();
}

pub(super) fn register_completed_switch_runtime(fixture: &RuntimeFixture, provider: &str) {
    let methods = [
        "initialize",
        "openThread",
        "resumeThread",
        "readHistory",
        "submitTurn",
        "control",
    ]
    .into_iter()
    .map(|kind| (kind.to_string(), json!([kind])))
    .collect::<serde_json::Map<String, Value>>();
    let manifest = PluginManifest::from_json_str(&json!({
        "id":provider,"version":"1.0.0","displayName":"Next fixture","binaryName":"node",
        "capabilities":{"agentProvider":true,"chatBackend":{"spawnable":true,"sessionConfig":true,"agentTasks":true},
            "agentRuntime":{"runtimeModes":[{"id":"auto-accept-edits","displayName":"Auto accept edits"}],
                "runtime":"stdio-jsonrpc","command":{"binary":"node","args":[]},
                "requestMethods":methods,"notificationMethods":[]}},
        "spawn":{"readinessStrategy":{"kind":"poll_shell_prompt","timeoutSecs":1},
            "taskDelivery":"inline_flag","systemPromptDelivery":{"kind":"cli_flag","flag":"--opaque-system"},
            "verifySubmit":{"kind":"none"}},
        "permissions":{"subprocess":{"allow":["node"]}}
    }).to_string()).unwrap();
    fixture
        .service
        .agent_registry
        .register_provider(Box::new(JsProvider::new(
            manifest,
            Arc::new(PluginVm::from_source(provider, SOURCE.into()).unwrap()),
        )));
}

pub(super) fn clear_switch_cooldown(fixture: &RuntimeFixture) {
    fixture
        .service
        .ext::<crate::service::agent_restart::RestartTracker>()
        .lock()
        .last_restart
        .remove(&fixture.tab);
}

pub(super) fn assert_completed_session_has_no_task_authority(
    fixture: &RuntimeFixture,
    task_id: &str,
) {
    use crate::service::plan::{
        interruption::TaskMutationAuthority, task_attempts::BoundWorkerReport,
    };
    let detail = fixture
        .service
        .plan_store
        .inspect_task_for_test(task_id)
        .unwrap();
    let current = fixture
        .service
        .mux
        .read()
        .get_tab(&fixture.tab)
        .cloned()
        .unwrap();
    let thread = current.agent_session_id.unwrap();
    let nonce = current.session_binding_nonce.unwrap();
    assert!(fixture
        .service
        .plan_store
        .acknowledge_live_task_attempt_for_tab_with_runtime_binding(
            &fixture.tab,
            Some(&detail.plan_id),
            Some(task_id),
            Some((&thread, &nonce)),
            8,
        )
        .is_err());
    let report = fixture
        .service
        .plan_store
        .ingest_bound_worker_report(
            &detail.plan_id,
            task_id,
            &crate::plan::AgentRef::from_tab(&fixture.tab),
            BoundWorkerReport {
                message: "forbidden completed-session report".into(),
                status: Some(codeterm_types::agent::ReportStatus::InReview),
                authority: Some(TaskMutationAuthority {
                    tab_id: fixture.tab.clone(),
                    session_id: thread,
                    session_binding_nonce: nonce,
                    owner_epoch: detail.task.interruption.owner_epoch,
                }),
                ..Default::default()
            },
            9,
        )
        .unwrap()
        .committed_review;
    assert!(!report);
    assert!(!fixture
        .service
        .signal_worker_interruption(
            &fixture.tab,
            crate::service::worker_interruption::WorkerInterruptionEvent::ProcessExited,
        )
        .unwrap());
    assert_eq!(
        fixture
            .service
            .plan_store
            .inspect_task_for_test(task_id)
            .unwrap()
            .task,
        detail.task
    );
}
