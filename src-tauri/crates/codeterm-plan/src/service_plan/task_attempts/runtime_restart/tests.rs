use super::*;

fn attempt() -> TaskExecutionAttempt {
    let mut attempt = TaskExecutionAttempt::default();
    attempt.id = "attempt".into();
    attempt.executor_tab_id = "tab".into();
    attempt.provider = Some("opaque-provider".into());
    attempt.session_id = Some("thread".into());
    attempt.session_binding_nonce = Some("old".into());
    attempt
}

fn request() -> RuntimeRestartBinding {
    RuntimeRestartBinding {
        plan_id: "plan".into(),
        task_id: "task".into(),
        attempt_id: "attempt".into(),
        executor_tab_id: "tab".into(),
        provider: "opaque-provider".into(),
        target_provider: None,
        old_thread: "thread".into(),
        old_nonce: "old".into(),
        new_thread: "thread".into(),
        new_nonce: "new".into(),
        owner_epoch: 1,
        mode: RuntimeRestartMode::Resume,
    }
}

#[test]
fn resume_restart_carries_the_same_attempt_to_the_new_launch_nonce() {
    let request = request();
    let before = attempt();

    let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new")
        .expect("the authenticated resumed launch carries its attempt");

    assert_eq!(after.id, before.id);
    assert_eq!(after.executor_tab_id, before.executor_tab_id);
    assert_eq!(after.session_id.as_deref(), Some("thread"));
    assert_eq!(after.session_binding_nonce.as_deref(), Some("new"));
}

#[test]
fn model_switch_resume_carries_the_same_attempt_to_the_new_launch_nonce() {
    let request = RuntimeRestartBinding {
        mode: RuntimeRestartMode::ModelSwitch,
        ..request()
    };
    let before = attempt();

    let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new")
        .expect("a model switch preserving the provider session keeps the attempt");

    assert_eq!(after.id, before.id);
    assert_eq!(after.provider, before.provider);
    assert_eq!(after.session_binding_nonce.as_deref(), Some("new"));
}

#[test]
fn runtime_restart_resolution_refuses_a_foreign_tab() {
    assert!(
        resolve_runtime_restart_attempt(&request(), &attempt(), "foreign-tab", "new").is_none()
    );
}

#[test]
fn provider_switch_carries_exact_attempt_and_refuses_stale_authority() {
    let mut request = RuntimeRestartBinding {
        mode: RuntimeRestartMode::ProviderSwitch,
        target_provider: Some("target-provider".into()),
        new_thread: "target-thread".into(),
        ..request()
    };
    let before = attempt();
    let after = resolve_runtime_restart_attempt(&request, &before, "tab", "new").unwrap();
    assert_eq!(after.id, before.id);
    assert_eq!(after.executor_tab_id, before.executor_tab_id);
    assert_eq!(after.provider.as_deref(), Some("target-provider"));
    assert_eq!(after.session_id.as_deref(), Some("target-thread"));
    assert!(resolve_runtime_restart_attempt(&request, &after, "tab", "new").is_none());
    request.mode = RuntimeRestartMode::Resume;
    assert!(!permits_rebind(&request, &before));
    request.mode = RuntimeRestartMode::ProviderSwitch;
    let mut closed = before;
    closed.closed_at = Some(1);
    assert!(!permits_rebind(&request, &closed));
    request.new_thread = request.old_thread.clone();
    assert!(!permits_rebind(&request, &closed));
}

#[test]
fn runtime_restart_resolution_refuses_the_old_nonce_after_carry() {
    let request = request();
    let rebound = resolve_runtime_restart_attempt(&request, &attempt(), "tab", "new")
        .expect("initial carry is authorized");

    assert!(resolve_runtime_restart_attempt(&request, &rebound, "tab", "old").is_none());
}

#[test]
fn exact_old_binding_and_explicit_fresh_authority_gate_thread_replacement() {
    let mut attempt = attempt();
    let request = request();
    assert!(permits_rebind(&request, &attempt));
    let mut fresh = request.clone();
    fresh.new_thread = "new-thread".into();
    assert!(!permits_rebind(&fresh, &attempt));
    fresh.mode = RuntimeRestartMode::AuthorizedFresh;
    assert!(permits_rebind(&fresh, &attempt));
    for invalid in [
        RuntimeRestartBinding {
            old_thread: "foreign".into(),
            ..request.clone()
        },
        RuntimeRestartBinding {
            old_nonce: "foreign".into(),
            ..request.clone()
        },
        RuntimeRestartBinding {
            executor_tab_id: "foreign".into(),
            ..request.clone()
        },
        RuntimeRestartBinding {
            attempt_id: "foreign".into(),
            ..request.clone()
        },
    ] {
        assert!(!permits_rebind(&invalid, &attempt));
    }
    attempt.closed_at = Some(1);
    assert!(permits_rebind(&request, &attempt));
    assert!(!permits_rebind(&fresh, &attempt));
    attempt.retired_at = Some(2);
    assert!(!permits_rebind(&request, &attempt));
}
