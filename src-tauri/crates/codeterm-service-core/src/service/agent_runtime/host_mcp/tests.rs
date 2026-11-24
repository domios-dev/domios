use super::*;

fn environment() -> HashMap<String, String> {
    HashMap::from([
        (
            "CODETERM_API_TOKEN".into(),
            "<credential-placeholder>".into(),
        ),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:43210".into()),
        ("CODETERM_TAB_ID".into(), "own".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "nonce".into()),
    ])
}

#[test]
fn agent_runtime_host_mcp_absent_without_minted_local_credential_or_declaration() {
    for (declared, minted) in [(false, false), (false, true), (true, false)] {
        assert!(descriptor(
            declared,
            minted,
            "own",
            "nonce",
            Some(std::path::PathBuf::from("/trusted/domios")),
            &environment(),
        )
        .is_none());
    }
    assert!(descriptor(true, true, "own", "nonce", None, &environment()).is_none());
    let actual = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some(std::path::PathBuf::from("/trusted/domios")),
        &environment(),
    )
    .unwrap();
    assert!(!format!("{actual:?}").contains("<credential-placeholder>"));
    assert_eq!(
        actual.descriptor(),
        serde_json::json!({"name":"domios","command":"/trusted/domios","args":["mcp","serve","--tab","own","--binding-nonce","nonce"],"env":[{"name":"CODETERM_API_TOKEN","value":"<credential-placeholder>"},{"name":"CODETERM_API_URL","value":"http://127.0.0.1:43210"},{"name":"CODETERM_SESSION_BINDING_NONCE","value":"nonce"},{"name":"CODETERM_TAB_ID","value":"own"}]})
    );
}

#[test]
fn agent_runtime_host_mcp_minted_local_refuses_foreign_stale_or_incomplete_environment() {
    use crate::service::tab_credentials::TabCaller;
    use std::collections::HashMap;
    let env = HashMap::from([
        ("CODETERM_API_TOKEN".into(), "opaque-test-credential".into()),
        ("CODETERM_API_URL".into(), "http://127.0.0.1:43210".into()),
        ("CODETERM_TAB_ID".into(), "own".into()),
        ("CODETERM_SESSION_BINDING_NONCE".into(), "nonce".into()),
    ]);
    let own = TabCaller {
        tab_id: "own".into(),
        binding_nonce: "nonce".into(),
    };
    assert!(minted_local(&env, &own, |_| Some(own.clone())));
    for caller in [
        TabCaller {
            tab_id: "foreign".into(),
            ..own.clone()
        },
        TabCaller {
            binding_nonce: "stale".into(),
            ..own.clone()
        },
    ] {
        assert!(!minted_local(&env, &own, |_| Some(caller)));
    }
    for key in [
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_TAB_ID",
        "CODETERM_SESSION_BINDING_NONCE",
    ] {
        let mut missing = env.clone();
        missing.remove(key);
        assert!(!minted_local(&missing, &own, |_| Some(own.clone())));
        let mut empty = env.clone();
        empty.insert(key.into(), String::new());
        assert!(!minted_local(&empty, &own, |_| Some(own.clone())));
    }
    for key in ["CODETERM_TAB_ID", "CODETERM_SESSION_BINDING_NONCE"] {
        let mut mismatch = env.clone();
        mismatch.insert(key.into(), "foreign".into());
        assert!(!minted_local(&mismatch, &own, |_| Some(own.clone())));
    }
    assert!(!minted_local(&env, &own, |_| None));
}

#[test]
fn agent_runtime_host_mcp_forwards_only_the_tab_task_identity() {
    let mut env = environment();
    env.insert("CODETERM_PLAN_ID".into(), "opaque-plan".into());
    env.insert("CODETERM_TASK_ID".into(), "opaque-task".into());
    env.insert("CODETERM_LAUNCH_FILE".into(), "opaque-file".into());
    env.insert("CODETERM_ROLE".into(), "opaque-role".into());
    let bound = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some("/trusted/domios".into()),
        &env,
    )
    .unwrap();
    assert_eq!(
        bound.env.keys().map(String::as_str).collect::<Vec<_>>(),
        [
            "CODETERM_API_TOKEN",
            "CODETERM_API_URL",
            "CODETERM_PLAN_ID",
            "CODETERM_SESSION_BINDING_NONCE",
            "CODETERM_TAB_ID",
            "CODETERM_TASK_ID",
        ]
    );
    assert_eq!(bound.env["CODETERM_TASK_ID"], "opaque-task");
    env.insert("CODETERM_TASK_ID".into(), " ".into());
    let unbound = descriptor(
        true,
        true,
        "own",
        "nonce",
        Some("/trusted/domios".into()),
        &env,
    )
    .unwrap();
    assert!(!unbound.env.contains_key("CODETERM_TASK_ID"));
}

#[test]
fn neighbor_reply_uses_current_runtime_cli_with_exact_instance_and_tab_launch() {
    let root = tempfile::tempdir().unwrap();
    let previous = root.path().join("prior worker CLI");
    let current = root.path().join("current worker CLI");
    std::fs::write(&current, "current").unwrap();
    let mut env = environment();
    let socket = codeterm_paths::namespaced_socket_display_path("domios-dev-opaque.sock");
    env.insert("CODETERM_SOCKET_PATH".into(), socket.clone());
    env.insert("CODETERM_OWNER_TOKEN".into(), "ambient-secret".into());
    let caller = TabCaller {
        tab_id: "own".into(),
        binding_nonce: "nonce".into(),
    };
    let scope = "opaque \"topic\"";
    let friendship = "a".repeat(64);
    let reply = NeighborReply {
        friendship: friendship.clone(),
        instructions: neighbor_reply_instructions(&friendship, scope),
    };
    assert!(neighbor_reply_descriptor(&reply, true, &caller, Some(previous), &env).is_err());
    assert!(
        neighbor_reply_descriptor(&reply, false, &caller, Some(current.clone()), &env).is_err()
    );
    assert!(reply.instructions.contains(&format!(
        "{NEIGHBOR_REPLY_MCP_TOOL} tool on MCP server {NEIGHBOR_REPLY_MCP_NAME}"
    )));
    assert!(reply
        .instructions
        .contains(&serde_json::to_string(scope).unwrap()));
    for old in [
        "account neighbors",
        "mem search",
        "Invoke-RestMethod",
        "curl",
        "--from-pad",
    ] {
        assert!(!reply.instructions.contains(old));
    }
    let server =
        neighbor_reply_descriptor(&reply, true, &caller, Some(current.clone()), &env).unwrap();
    assert_eq!(server.name, NEIGHBOR_REPLY_MCP_NAME);
    assert_eq!(PathBuf::from(&server.command), current);
    assert_eq!(
        server.args,
        ["mcp", "serve", "--tab", "own", "--binding-nonce", "nonce"]
    );
    assert_eq!(
        server.env,
        std::collections::BTreeMap::from([
            ("CODETERM_API_URL".into(), env["CODETERM_API_URL"].clone()),
            ("CODETERM_SOCKET_PATH".into(), socket),
            ("CODETERM_TAB_ID".into(), "own".into()),
            ("CODETERM_SESSION_BINDING_NONCE".into(), "nonce".into()),
            (NEIGHBOR_REPLY_FRIENDSHIP_ENV.into(), friendship),
        ])
    );
    assert!(server.http.is_none());
}

#[test]
fn neighbor_reply_selects_transport_from_the_bound_sandbox_and_cli_capability() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};

    let (mut mux, root) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&root).unwrap();
    let friendship = "a".repeat(64);
    tab.session_binding_nonce = Some("current".into());
    tab.neighbor_authority = Some(NeighborAuthority {
        uid: "opaque".into(),
        friendship_id: friendship.clone(),
        root_tab_id: None,
    });
    let read_only = codeterm_types::plan::ExecutionSandboxPolicy {
        intent: ExecutionIntent::ReadOnly,
        cwd: "/friend".into(),
        launch_args: vec![],
        runtime_mode: Some("read-only-with-writable-roots".into()),
        writable_roots: vec!["/own-pad".into()],
        environment: Default::default(),
    };
    let folder = codeterm_types::plan::ExecutionSandboxPolicy {
        intent: ExecutionIntent::Mutating,
        runtime_mode: Some("workspace-write".into()),
        ..read_only.clone()
    };
    let mut context = RuntimeContext {
        binding_nonce: "current".into(),
        settings: codeterm_plugin::agent_runtime::RuntimeSettings {
            network_access: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let restricted_cli = RestrictedCliCapability::ExecpolicyServiceOperations {
        metadata_directory: ".fixture".into(),
    };
    for platform in ["macos", "windows", "linux"] {
        tab.restorable_launch.execution_sandbox = Some(read_only.clone());
        assert_eq!(
            sandboxed_reply_friendship(tab, &context, platform, Some(&restricted_cli)),
            Some(friendship.as_str())
        );
        tab.restorable_launch.execution_sandbox = None;
        tab.restorable_launch.permission_bypass_requested = true;
        assert!(
            sandboxed_reply_friendship(tab, &context, platform, Some(&restricted_cli)).is_none()
        );
        tab.restorable_launch.permission_bypass_requested = false;
        tab.restorable_launch.execution_sandbox = Some(folder.clone());
        let expected = (platform != "windows").then_some(friendship.as_str());
        assert_eq!(
            sandboxed_reply_friendship(tab, &context, platform, Some(&restricted_cli)),
            expected
        );
        assert_eq!(
            sandboxed_reply_friendship(tab, &context, platform, None),
            Some(friendship.as_str())
        );
    }
    tab.restorable_launch
        .execution_sandbox
        .as_mut()
        .unwrap()
        .runtime_mode = Some("opaque-confined-mode".into());
    assert_eq!(
        sandboxed_reply_friendship(tab, &context, "windows", Some(&restricted_cli)),
        Some(friendship.as_str())
    );
    context.settings.network_access = Some(true);
    assert!(sandboxed_reply_friendship(tab, &context, "macos", Some(&restricted_cli)).is_none());
    context.settings.network_access = Some(false);
    context.binding_nonce = "retired".into();
    assert!(sandboxed_reply_friendship(tab, &context, "macos", Some(&restricted_cli)).is_none());
    context.binding_nonce = "current".into();
    tab.neighbor_authority.as_mut().unwrap().root_tab_id = Some("parent".into());
    assert!(sandboxed_reply_friendship(tab, &context, "macos", Some(&restricted_cli)).is_none());
}
