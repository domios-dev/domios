use super::{
    capture_tab_launch_parameters, captured_runtime_launch_options, RestorableLaunchCapture,
};

#[test]
fn restored_target_uses_only_newest_own_lineage() {
    use crate::multiplexer::types::PriorBindingNonce;
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        session_id: Some("old-own-session".into()),
        resume_target_session_id: None,
        nonce: "old".into(),
    });
    assert_eq!(super::restored_session_target(tab), Some("old-own-session"));
    tab.agent_session_id = Some("current-own-session".into());
    assert_eq!(
        super::restored_session_target(tab),
        Some("current-own-session")
    );
    tab.agent_session_id = None;
    tab.prior_session_binding_nonces.push(PriorBindingNonce {
        session_id: None,
        resume_target_session_id: None,
        nonce: "unbound-fresh-launch".into(),
    });
    assert_eq!(super::restored_session_target(tab), None);
}

#[test]
fn capture_keeps_worker_identity_without_assignment() {
    use std::sync::{mpsc, Arc};
    use std::time::Duration;

    const IDENTITY: &str = "opaque-worker-identity-02a1";
    const ASSIGNMENT: &str = "opaque-worker-assignment-f3b2";
    const CAPTURE_THREAD: &str = "restorable-launch-capture";
    const DEADLINE: Duration = Duration::from_secs(5);

    let (state, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let mux = Arc::new(codeterm_util::lock_hold::TimedRwLock::new("mux", state));
    let (locked_tx, locked_rx) = mpsc::channel();
    let (capture_tx, capture_rx) = mpsc::channel();
    let writer_mux = Arc::clone(&mux);
    let writer_tab = tab_id.clone();
    let writer = std::thread::spawn(move || {
        let mut mux = writer_mux.write();
        locked_tx.send(()).unwrap();
        capture_rx.recv_timeout(DEADLINE).unwrap();
        mux.get_tab_mut(&writer_tab)
            .unwrap()
            .restorable_launch
            .account_id = Some("account-capture".into());
    });
    locked_rx.recv_timeout(DEADLINE).unwrap();

    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("capture-provider")
            .with_session_config_delivery(),
    ));
    let task = crate::service::plan::worker_identity::attach_worker_identity(IDENTITY, ASSIGNMENT);
    let capture_mux = Arc::clone(&mux);
    let capture_tab = tab_id.clone();
    let (launch_tx, launch_rx) = mpsc::channel();
    let capture = std::thread::Builder::new()
        .name(CAPTURE_THREAD.into())
        .spawn(move || {
            capture_tx.send(()).unwrap();
            let launch = capture_tab_launch_parameters(
                &capture_mux,
                &capture_tab,
                &registry,
                RestorableLaunchCapture {
                    provider_id: "capture-provider",
                    requested_model: None,
                    model: None,
                    reasoning_effort: None,
                    args: None,
                    agent_name: None,
                    task: Some(&task),
                    role: Some("Worker"),
                    role_profile: None,
                    permission_bypass_requested: false,
                },
            );
            launch_tx.send(launch).unwrap();
        })
        .unwrap();
    let launch = launch_rx
        .recv_timeout(DEADLINE)
        .expect("launch capture must finish");
    writer.join().unwrap();
    capture.join().unwrap();

    #[cfg(debug_assertions)]
    {
        let violations = codeterm_util::lock_hold::order::violations_mentioning(CAPTURE_THREAD);
        assert!(violations.is_empty(), "{violations:#?}");
    }
    assert!(launch.captured);
    assert_eq!(launch.account_id.as_deref(), Some("account-capture"));
    let identity = launch.identity_instructions.unwrap();
    assert!(identity.contains(IDENTITY));
    assert!(!identity.contains(ASSIGNMENT));
    let state = mux.read();
    let persisted = &state.get_tab(&tab_id).unwrap().restorable_launch;
    assert_eq!(persisted.account_id.as_deref(), Some("account-capture"));
    assert_eq!(
        persisted.identity_instructions.as_deref(),
        Some(identity.as_str())
    );
}

#[test]
fn restored_launch_preserves_context_and_skills_without_replaying_task() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "resume-fixture",
    )));
    let files = crate::plan::TaskFiles {
        edit: vec!["/opaque/new.rs".into()],
        read: vec!["/opaque/context.md".into()],
    };
    let input = super::restored_fresh_launch_input(
        &registry,
        crate::service::SpawnAgentParams {
            agent: Some("resume-fixture".into()),
            cwd: Some("/opaque/worker".into()),
            account_id: Some("account-stale-request".into()),
            ..Default::default()
        },
        crate::multiplexer::types::RestorableLaunchParameters {
            captured: true,
            account_id: Some("account-restored".into()),
            context_files: Some(files.clone()),
            context_skills: Some("opaque skills".into()),
            ..Default::default()
        },
    );
    assert_eq!(input.params.account_id.as_deref(), Some("account-restored"));
    assert_eq!(
        input.launch_parameters.account_id.as_deref(),
        Some("account-restored")
    );
    assert_eq!(input.params.context_files, Some(files));
    assert_eq!(
        input.params.context_skills.as_deref(),
        Some("opaque skills")
    );
    assert_eq!(input.params.cwd.as_deref(), Some("/opaque/worker"));
    assert!(input.params.task.is_none());
}
#[test]
fn recapture_keeps_runtime_options_only_for_the_exact_provider_binding() {
    let options = crate::multiplexer::types::RestorableRuntimeLaunchOptions {
        provider_id: "fixture-provider".into(),
        binding_nonce: "own".into(),
        mode: "auto-accept-edits".into(),
        ..Default::default()
    };
    assert_eq!(
        captured_runtime_launch_options("fixture-provider", Some("own"), Some(&options)),
        Some(options.clone())
    );
    assert!(
        captured_runtime_launch_options("fixture-provider", Some("new"), Some(&options)).is_none()
    );
    assert!(
        captured_runtime_launch_options("other-provider", Some("own"), Some(&options)).is_none()
    );
}
