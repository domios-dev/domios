use super::*;

#[test]
fn verified_rebind_preserves_same_session_nonce_authority() {
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("codex".into());
        tab.agent_session_id = Some("sid-a".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.reported_session_binding_nonce = Some("launch-nonce".into());
    }

    let evidence = current_identity_evidence(&mux, &tab_id);
    let claim = crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry(&["codex"]),
        &tab_id,
        "codex",
        "sid-a",
        Some("launch-nonce"),
        &evidence,
    )
    .unwrap();

    assert!(claim.nonce_verified);
    assert!(!claim.changed);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .reported_session_binding_nonce
            .as_deref(),
        Some("launch-nonce")
    );
}

#[test]
fn validated_rebind_bypasses_a_stale_label_without_prevalidation_write() {
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("sid-a".into());
    }
    let evidence = current_identity_evidence(&mux, &tab_id);

    crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry(&["codex"]),
        &tab_id,
        "codex",
        "sid-b",
        None,
        &evidence,
    )
    .unwrap();

    let mux = mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("sid-b"));
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert!(tab.agent_type_locked);
}

#[test]
fn validated_rebind_refuses_a_changed_tab_evidence_generation() {
    let (mux, tab_id) = shared_mux();
    {
        let mut mux = mux.write();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.cwd = Some("/tmp/project".into());
        tab.agent_type = Some("claude".into());
        tab.agent_type_locked = true;
        tab.agent_session_id = Some("sid-a".into());
        tab.session_binding_nonce = Some("launch-nonce".into());
        tab.reported_session_binding_nonce = Some("launch-nonce".into());
    }
    let evidence = current_identity_evidence(&mux, &tab_id);
    mux.write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .reported_session_binding_nonce = None;

    let error = crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry(&["codex"]),
        &tab_id,
        "codex",
        "sid-b",
        None,
        &evidence,
    )
    .unwrap_err();

    assert_eq!(error.0, StatusCode::CONFLICT);
    let mux = mux.read();
    let tab = mux.get_tab(&tab_id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("sid-a"));
    assert_eq!(tab.agent_type.as_deref(), Some("claude"));
    assert!(tab.agent_type_locked);
}

#[test]
fn session_binding_deferral_guard_still_guards_hook_and_rebind() {
    let (_temp, mux, tab_id, registry, _new_path) = materializing_binding_fixture();
    let hook = bind_reported_session(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "new",
        Some("launch-nonce"),
    )
    .unwrap_err();
    let verb = crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "new",
        None,
        &current_identity_evidence(&mux, &tab_id),
    )
    .unwrap_err();
    assert_eq!(hook.0, StatusCode::CONFLICT);
    assert_eq!(verb.0, StatusCode::CONFLICT);
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("old")
    );
}

#[test]
fn session_binding_deferral_loses_to_legitimate_binding() {
    let (temp, mux, tab_id, registry, new_path) = materializing_binding_fixture();
    let pending = Arc::new(
        crate::service::session_binding_deferral::DeferredSessionBindingRegistry::default(),
    );
    let callback_pending = pending.clone();
    let callback_mux = mux.clone();
    let (tx, rx) = std::sync::mpsc::channel();
    let binding = pending
        .defer(
            &tab_id,
            "codex",
            "new",
            "old",
            new_path.clone(),
            Arc::new(move |tab, generation| {
                let held = callback_pending.get(&tab, generation).unwrap();
                let _ = tx.send(deferred_binding_still_applies(&callback_mux, &held));
            }),
        )
        .unwrap();
    std::fs::write(temp.path().join("legitimate.jsonl"), "{}").unwrap();
    crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "legitimate",
        None,
        &current_identity_evidence(&mux, &tab_id),
    )
    .unwrap();
    assert!(!deferred_binding_still_applies(&mux, &binding));
    std::fs::write(new_path, "{}").unwrap();
    assert!(!rx
        .recv_timeout(Duration::from_secs(2))
        .expect("materialization event"));
    assert_eq!(
        mux.read()
            .get_tab(&tab_id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("legitimate")
    );
}

#[test]
fn session_binding_deferral_rebind_preserves_agent_process() {
    let (_temp, mux, tab_id, registry, new_path) = materializing_binding_fixture();
    std::fs::write(new_path, "{}").unwrap();
    let pty_before = mux.read().get_tab(&tab_id).unwrap().pty_id.clone();
    crate::api::session_binding_routes::rebind_session_core(
        &mux,
        &registry,
        &tab_id,
        "codex",
        "new",
        None,
        &current_identity_evidence(&mux, &tab_id),
    )
    .unwrap();
    let guard = mux.read();
    let tab = guard.get_tab(&tab_id).unwrap();
    assert_eq!(tab.pty_id, pty_before);
    assert_eq!(tab.agent_session_id.as_deref(), Some("new"));
}
