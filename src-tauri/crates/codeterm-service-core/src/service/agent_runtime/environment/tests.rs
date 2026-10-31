use super::*;

#[test]
fn restored_chat_launch_environment_mints_current_credentials_and_rejects_the_previous_daemon_token(
) {
    use crate::multiplexer::{state::MultiplexerState, types::TabBacking};
    use crate::service::test_fixture::ServiceFixture;

    let (mut mux, id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("fixture-runtime".into());
    tab.agent_session_id = Some("own-thread".into());
    tab.session_binding_nonce = Some("saved-nonce".into());
    tab.reported_session_binding_nonce = Some("saved-nonce".into());
    tab.backing = TabBacking::ChatBackend {
        plugin_id: "fixture-runtime".into(),
        session_id: "own-thread".into(),
    };
    let previous = ServiceFixture::new(mux);
    let old_env = compose(&previous.service, &id, "fixture-runtime", "saved-nonce");
    let old_token = &old_env["CODETERM_API_TOKEN"];
    let restored = ServiceFixture::new(MultiplexerState::from_session_snapshot(
        &previous.service.mux.read().to_session_snapshot(),
    ));
    let nonce = restored
        .service
        .mux
        .read()
        .get_tab(&id)
        .unwrap()
        .session_binding_nonce
        .clone()
        .unwrap();
    let env = compose(&restored.service, &id, "fixture-runtime", &nonce);
    let current_token = &env["CODETERM_API_TOKEN"];
    assert_ne!(current_token, old_token);
    assert!(restored
        .service
        .authenticate_local_credential(old_token)
        .is_err());
    let caller = restored
        .service
        .authenticate_local_credential(current_token)
        .unwrap()
        .unwrap();
    assert_eq!(caller.tab_id, id);
    assert_eq!(caller.binding_nonce, nonce);
    assert_eq!(env["CODETERM_TAB_ID"], id);
    assert_eq!(env["CODETERM_SESSION_BINDING_NONCE"], nonce);
    assert_eq!(
        compose(&restored.service, &id, "fixture-runtime", &nonce)["CODETERM_API_TOKEN"],
        *current_token
    );

    let next_nonce = restored.service.rotate_session_binding_nonce(&id);
    let next_env = compose(&restored.service, &id, "fixture-runtime", &next_nonce);
    assert!(restored
        .service
        .authenticate_local_credential(current_token)
        .is_err());
    assert_ne!(next_env["CODETERM_API_TOKEN"], *current_token);
    assert_eq!(
        restored
            .service
            .authenticate_local_credential(&next_env["CODETERM_API_TOKEN"])
            .unwrap()
            .unwrap()
            .binding_nonce,
        next_nonce
    );
    assert_eq!(
        restored
            .service
            .mux
            .read()
            .get_tab(&id)
            .unwrap()
            .agent_session_id
            .as_deref(),
        Some("own-thread")
    );
}

#[test]
fn host_identity_wins_and_absent_roles_cannot_leak_from_extra_env() {
    let extra = HashMap::from([
        ("CODETERM_TASK_ID".into(), "foreign".into()),
        ("CODETERM_ROLE".into(), "foreign".into()),
        ("CODETERM_API_TOKEN".into(), "foreign".into()),
        ("PATH".into(), "owned-path".into()),
        ("CODETERM_CLI_PATH".into(), "owned-cli".into()),
        ("HIDDEN_WORKER_CONTEXT".into(), "owned-context".into()),
    ]);
    let owned = HashMap::from([
        ("CODETERM_TASK_ID".into(), "own-task".into()),
        ("CODETERM_API_TOKEN".into(), "own-token".into()),
        ("CODETERM_WORKER_ID".into(), "own-worker".into()),
    ]);
    let env = compose_owned_environment(extra, owned);
    assert_eq!(env.get("CODETERM_TASK_ID").unwrap(), "own-task");
    assert_eq!(env.get("CODETERM_API_TOKEN").unwrap(), "own-token");
    assert_eq!(env.get("CODETERM_WORKER_ID").unwrap(), "own-worker");
    assert!(!env.contains_key("CODETERM_ROLE"));
    assert_eq!(env.get("PATH").unwrap(), "owned-path");
    assert_eq!(env.get("CODETERM_CLI_PATH").unwrap(), "owned-cli");
    assert_eq!(env.get("HIDDEN_WORKER_CONTEXT").unwrap(), "owned-context");
}

#[test]
fn lifecycle_cli_and_exact_caller_identity_override_forged_launch_extras() {
    use crate::service::instance_env::InstanceEnv;
    use crate::service::pty_io::tab_env::{compose_launch_env, LaunchSurface, TabLaunchEnv};

    let instance = InstanceEnv {
        socket_path: "own-socket".into(),
        api_url: "http://127.0.0.1:17686".into(),
        api_port: "17686".into(),
        cli_path: Some("private-worker/codeterm".into()),
    };
    let tab = TabLaunchEnv {
        tab_id: "own-tab".into(),
        pad_dir: Some("own-pad".into()),
        session_binding_nonce: "own-session-nonce".into(),
        shell_nonce: "own-shell-nonce".into(),
        api_token: Some("own-token".into()),
        identity: HashMap::new(),
        task: HashMap::new(),
    };
    let owned = compose_launch_env(&instance, Some(&tab), LaunchSurface::Service);
    let mut extra = owned
        .keys()
        .map(|key| (key.clone(), "foreign".into()))
        .collect::<HashMap<_, _>>();
    extra.insert("CODETERM_ROLE".into(), "foreign".into());
    let composed = compose_owned_environment(extra, owned.clone());
    assert_eq!(composed, owned);
    assert_eq!(composed["CODETERM_CLI_PATH"], "private-worker/codeterm");
    assert_eq!(composed["CODETERM_TAB_ID"], "own-tab");
    assert_eq!(
        composed["CODETERM_SESSION_BINDING_NONCE"],
        "own-session-nonce"
    );
    assert_eq!(composed["CODETERM_API_TOKEN"], "own-token");
}
