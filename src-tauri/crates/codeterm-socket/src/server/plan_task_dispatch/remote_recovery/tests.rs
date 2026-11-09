use super::*;

#[test]
fn remote_recovery_requires_ack_deadline_or_explicit_replacement() {
    let mut attempt = RemoteRecoveryAttemptRead {
        binding: RemoteWorkerBindingRead {
            id: "attempt".into(),
            executor_tab_id: "worker".into(),
            mesh_peer: Some("peer".into()),
            session_binding_nonce: Some("nonce".into()),
            closed_at: None,
            retired_at: None,
        },
        acknowledged_at: None,
        assigned_at: Some(1),
        remote_dispatch_state: Some(
            codeterm_service_core::plan::RemoteDispatchState::AwaitingWorkerAck,
        ),
    };
    let deadline = codeterm_service_core::service::worker_ack::REMOTE_WORKER_ACK_DEADLINE
        .as_millis() as u64
        + 1;
    assert_eq!(
        remote_recovery_decision(&attempt, deadline - 1, false),
        (false, false)
    );
    assert_eq!(
        remote_recovery_decision(&attempt, deadline, false),
        (true, false)
    );
    assert_eq!(remote_recovery_decision(&attempt, 2, true), (false, true));
    attempt.acknowledged_at = Some(2);
    assert_eq!(
        remote_recovery_decision(&attempt, deadline, true),
        (false, false)
    );
}

#[test]
fn remote_respawn_keeps_peer_gates_and_respects_model_override() {
    let attempt = TaskExecutionAttempt {
        mesh_peer: Some("paired-peer-id".into()),
        provider: Some("provider".into()),
        model: Some("original-model".into()),
        reasoning_effort: Some("high".into()),
        authorized_gates: Some(vec!["gate".into()]),
        ..Default::default()
    };
    let params = remote_respawn_params(
        &Some(json!({"plan_id": "plan", "task_id": "task", "model": "replacement-model"})),
        &attempt,
    );
    assert_eq!(params["mesh_peer"], "paired-peer-id");
    assert_eq!(params["execution_intent"], "mutating");
    assert_eq!(params["agent"], "provider");
    assert_eq!(params["model"], "replacement-model");
    assert_eq!(params["reasoning_effort"], "high");
    assert_eq!(params["authorized_gates"], json!(["gate"]));
    assert_eq!(params["plan_id"], "plan");
    let rest_params =
        remote_respawn_params(&Some(json!({"provider": null, "model": null})), &attempt);
    assert_eq!(rest_params["agent"], "provider");
    assert_eq!(rest_params["model"], "original-model");
}

#[test]
fn mesh_profile_refusals_precede_plan_lookup_and_executor_recovery() {
    use codeterm_service_core::multiplexer::state::MultiplexerState;
    use codeterm_service_core::service::test_fixture::ServiceFixture;
    use std::sync::Arc;
    let (mux, _) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let svc = Arc::new(fixture.service.clone());
    let before = svc.mux.read().snapshot().tabs.len();
    let error =
        super::super::remote::dispatch(&Some(json!({"launch_profile":"selected"})), &svc, 1)
            .unwrap_err();
    assert_eq!(error.code, -32602);
    assert!(error
        .message
        .contains("launch_profile selection is local-only"));
    for (params, profile) in [
        (Some(json!({"launch_profile":"selected"})), None),
        (Some(json!({})), Some("selected".to_owned())),
    ] {
        let attempt = TaskExecutionAttempt {
            launch_profile: profile,
            ..Default::default()
        };
        let error =
            respawn_remote(&params, &svc, "absent-plan", "absent-task", &attempt, 1).unwrap_err();
        assert_eq!(error.code, -32602);
        assert!(error.message.contains("recorded profile cannot be dropped"));
    }
    assert_eq!(svc.mux.read().snapshot().tabs.len(), before);
}
