use super::*;
use codeterm_plugin::agent_runtime::{AgentRuntimeCommand, AgentRuntimeProtocol, RpcCall};

#[cfg(target_os = "macos")]
struct ProcessBoundaryTestLogger;

#[cfg(target_os = "macos")]
impl log::Log for ProcessBoundaryTestLogger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Debug
            && metadata
                .target()
                .contains("service::agent_runtime::process_boundary::unix")
    }

    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("{}", record.args());
        }
    }

    fn flush(&self) {}
}

#[cfg(target_os = "macos")]
static PROCESS_BOUNDARY_TEST_LOGGER: ProcessBoundaryTestLogger = ProcessBoundaryTestLogger;

#[cfg(target_os = "macos")]
fn init_process_boundary_test_logger() {
    let _ = log::set_logger(&PROCESS_BOUNDARY_TEST_LOGGER);
    log::set_max_level(log::LevelFilter::Debug);
}

fn capability() -> AgentRuntimeCapability {
    AgentRuntimeCapability {
        restricted_cli: None,
        runtime_modes: Vec::new(),
        service_tiers: Vec::new(),
        background_processes: false,
        goals: None,
        thread_history: None,
        stored_history: false,
        usage: false,
        reset_credits_min_version: None,
        runtime: AgentRuntimeProtocol::StdioJsonrpc,
        command: AgentRuntimeCommand {
            binary: "node".into(),
            args: vec![format!(
                "{}/../../../tests/fixtures/agent-runtime/fake-peer.mjs",
                crate::test_support::fixture_manifest_dir().display()
            )],
        },
        inherited_environment_exclusions: Vec::new(),
        login: None,
        request_methods: std::collections::BTreeMap::from([(
            "control".into(),
            vec![
                "fixture/environment".into(),
                "fixture/cli-resolution".into(),
                "fixture/exit".into(),
                "fixture/drop".into(),
            ],
        )]),
        notification_methods: Vec::new(),
    }
}
#[test]
fn stderr_tail_stays_bounded() {
    let mut output = vec![b'a'; 8192];
    append_stderr(&mut output, b"last");
    assert_eq!(output.len(), 8192);
    assert!(output.ends_with(b"last"));
}

#[tokio::test]
async fn finish_confirms_command_parent_and_child_exit_by_retained_handles() {
    assert_tree_exit("finish").await;
}

#[tokio::test]
async fn stop_confirms_command_parent_and_child_exit_by_retained_handles() {
    assert_tree_exit("stop").await;
}

#[tokio::test]
async fn drop_confirms_command_parent_and_child_exit_by_retained_handles() {
    assert_tree_exit("drop").await;
}

#[tokio::test]
async fn finish_retires_grandchild_after_intermediary_exit() {
    assert_tree_exit("orphan").await;
}

#[tokio::test]
async fn natural_server_exit_retires_command_tree() {
    assert_tree_exit("natural-exit").await;
}

async fn assert_tree_exit(action: &str) {
    let dir = tempfile::tempdir().unwrap();
    let mut cap = capability();
    cap.command.args = vec![format!(
        "{}/../../../tests/fixtures/agent-runtime/owned-tree.mjs",
        crate::test_support::fixture_manifest_dir().display()
    )];
    let process = spawn(
        Path::new("node"),
        &cap,
        dir.path().to_str().unwrap(),
        HashMap::new(),
        "fixture-owned-tree",
    )
    .unwrap();
    let result = process
        .client
        .request_guarded(
            &RpcCall {
                target: None,
                method: "fixture/tree".into(),
                params: serde_json::json!({"orphan": action == "orphan"}),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let pids: Vec<u32> = serde_json::from_value(result["pids"].clone()).unwrap();
    assert_eq!(pids.len(), 2);
    let members = process.boundary.as_ref().unwrap().snapshot().unwrap();
    let live_pids = if action == "orphan" {
        &pids[1..]
    } else {
        &pids[..]
    };
    for pid in live_pids {
        let member = members
            .iter()
            .find(|member| member.pid == *pid)
            .unwrap_or_else(|| {
                panic!(
                    "{action}: command {pid} missing from boundary members {:?}",
                    members.iter().map(|member| member.pid).collect::<Vec<_>>()
                )
            });
        assert!(
            !member.exited().unwrap(),
            "{action}: command {pid} must start alive"
        );
    }
    let mut ended = process.ended.clone();
    match action {
        "finish" => process.finish().await.unwrap(),
        "stop" => process.stop(),
        "drop" => drop(process),
        "orphan" => process.finish().await.unwrap(),
        "natural-exit" => {
            let _ = process
                .client
                .request_guarded(
                    &RpcCall {
                        target: None,
                        method: "fixture/exit".into(),
                        params: serde_json::json!({}),
                    },
                    None,
                )
                .await;
        }
        _ => unreachable!(),
    }
    tokio::time::timeout(Duration::from_secs(10), async {
        while ended.borrow_and_update().is_none() {
            ended.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(*ended.borrow(), Some(RuntimeFailure::ProcessExited { .. })),
        "{action}: {:?}",
        *ended.borrow()
    );
    if action == "natural-exit" {
        assert!(matches!(
            *ended.borrow(),
            Some(RuntimeFailure::ProcessExited {
                requested: false,
                ..
            })
        ));
    }
    for member in members {
        assert!(
            member.exited().unwrap(),
            "{action}: {} survived",
            member.pid
        );
    }
}
#[tokio::test]
async fn own_tab_environment_reaches_the_child_and_its_shell_commands() {
    let dir = tempfile::tempdir().unwrap();
    let env = HashMap::from([
        ("CODETERM_TAB_ID".into(), "fixture-tab".into()),
        ("CODETERM_PLAN_ID".into(), "fixture-plan".into()),
        ("CODETERM_TASK_ID".into(), "fixture-task".into()),
        ("CODETERM_WORKER_ID".into(), "fixture-hidden-worker".into()),
        ("CODETERM_ROLE".into(), "fixture-role".into()),
        ("CODETERM_TEAM_ID".into(), "fixture-team".into()),
        ("CODETERM_PARENT_TAB_ID".into(), "fixture-parent".into()),
        ("CODETERM_SOCKET_PATH".into(), "fixture-socket".into()),
        ("CODETERM_ATTEMPT_ID".into(), "fixture-attempt".into()),
        ("CODETERM_API_URL".into(), "fixture-api".into()),
        ("CODETERM_API_TOKEN".into(), "fixture-token".into()),
        ("CODETERM_PAD_DIR".into(), "fixture-pad".into()),
        (
            "CODETERM_SESSION_BINDING_NONCE".into(),
            "fixture-nonce".into(),
        ),
    ]);
    let process = spawn(
        Path::new("node"),
        &capability(),
        dir.path().to_str().unwrap(),
        env.clone(),
        "fixture-tab",
    )
    .unwrap();
    let result = process
        .client
        .request_guarded(
            &RpcCall {
                target: None,
                method: "fixture/environment".into(),
                params: serde_json::json!({"keys":env.keys().collect::<Vec<_>>()}),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result["environment"], serde_json::to_value(env).unwrap());
    process.finish().await.unwrap();
}

fn write_cli_probe(directory: &Path, artifact: &str) {
    std::fs::create_dir_all(directory).unwrap();
    let peer = Path::new(&capability().command.args[0])
        .canonicalize()
        .unwrap();
    for alias in ["domios", "codeterm"] {
        #[cfg(windows)]
        let (name, script) = (
            format!("{alias}.cmd"),
            format!(
                "@node \"{}\" --cli-probe {artifact} \"%~f0\"\r\n",
                peer.display()
            ),
        );
        #[cfg(unix)]
        let (name, script) = (
            alias.to_string(),
            format!(
                "#!/bin/sh\nexec node '{}' --cli-probe {artifact} \"$0\"\n",
                peer.to_str().unwrap().replace('\'', "'\\''")
            ),
        );
        let path = directory.join(name);
        std::fs::write(&path, script).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
}

#[tokio::test]
async fn lifecycle_cli_pin_controls_bare_aliases_in_child_and_grandchild_shells() {
    use crate::service::instance_env::InstanceEnv;
    use crate::service::pty_io::tab_env::{compose_launch_env, LaunchSurface, TabLaunchEnv};

    let dir = tempfile::tempdir().unwrap();
    let pinned = dir.path().join("private worker");
    let stale = dir.path().join("stale bin");
    write_cli_probe(&pinned, "pinned");
    write_cli_probe(&stale, "stale");
    let suffix = if cfg!(windows) { ".cmd" } else { "" };
    let instance = InstanceEnv {
        socket_path: "fixture-own-socket".into(),
        api_url: "http://127.0.0.1:17686".into(),
        api_port: "17686".into(),
        cli_path: Some(pinned.join(format!("codeterm{suffix}"))),
    };
    let tab = TabLaunchEnv {
        tab_id: "fixture-own-tab".into(),
        pad_dir: None,
        session_binding_nonce: "fixture-own-nonce".into(),
        shell_nonce: "fixture-own-shell-nonce".into(),
        api_token: Some("fixture-own-token".into()),
        identity: HashMap::new(),
        task: HashMap::new(),
    };
    let mut env = compose_launch_env(&instance, Some(&tab), LaunchSurface::Service);
    let mut identity = env.clone();
    identity.remove("CODETERM_SHELL_NONCE");
    let inherited = std::env::var_os("PATH").unwrap();
    let inherited_path = std::env::join_paths(
        std::iter::once(stale.clone()).chain(std::env::split_paths(&inherited)),
    )
    .unwrap()
    .into_string()
    .unwrap();
    env.insert("PATH".into(), inherited_path.clone());
    let process = spawn(
        Path::new("node"),
        &capability(),
        dir.path().to_str().unwrap(),
        env,
        "fixture-own-tab",
    )
    .unwrap();
    let result = process
        .client
        .request_guarded(
            &RpcCall {
                target: None,
                method: "fixture/cli-resolution".into(),
                params: serde_json::json!({"inheritedPath": inherited_path}),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();
    process.finish().await.unwrap();
    for (surface, artifact, directory) in [
        ("baseline", "stale", &stale),
        ("child", "pinned", &pinned),
        ("grandchild", "pinned", &pinned),
    ] {
        let probes = result[surface].as_array().unwrap();
        assert_eq!(probes.len(), 2, "{surface}");
        for (probe, alias) in probes.iter().zip(["domios", "codeterm"]) {
            assert_eq!(probe["artifact"], artifact, "{surface}: {alias}");
            assert_eq!(
                Path::new(probe["executable"].as_str().unwrap())
                    .canonicalize()
                    .unwrap(),
                directory
                    .join(format!("{alias}{suffix}"))
                    .canonicalize()
                    .unwrap(),
                "{surface}: {alias}"
            );
            assert_eq!(
                probe["environment"],
                serde_json::to_value(&identity).unwrap()
            );
        }
    }
}
#[tokio::test]
async fn child_exit_reports_status_and_stderr() {
    let dir = tempfile::tempdir().unwrap();
    let process = spawn(
        Path::new("node"),
        &capability(),
        dir.path().to_str().unwrap(),
        HashMap::new(),
        "fixture-tab",
    )
    .unwrap();
    let _ = process
        .client
        .request_guarded(
            &RpcCall {
                target: None,
                method: "fixture/exit".into(),
                params: serde_json::json!({"code":23}),
            },
            None,
        )
        .await;
    let mut ended = process.ended.clone();
    tokio::time::timeout(Duration::from_secs(5), async {
        while ended.borrow_and_update().is_none() {
            ended.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert!(
        matches!(&*ended.borrow(),Some(RuntimeFailure::ProcessExited{code:Some(23),stderr,..}) if stderr=="opaque fixture stderr")
    );
}

// A live Node process on Windows keeps its stdout pipe open after closing fd 1, so the fixture cannot drop it.
#[cfg(unix)]
#[tokio::test]
async fn pipe_drop_from_a_live_peer_is_not_success() {
    #[cfg(target_os = "macos")]
    init_process_boundary_test_logger();

    let dir = tempfile::tempdir().unwrap();
    let process = spawn(
        Path::new("node"),
        &capability(),
        dir.path().to_str().unwrap(),
        HashMap::new(),
        "fixture-tab",
    )
    .unwrap();
    assert_eq!(
        process
            .client
            .request_guarded(
                &RpcCall {
                    target: None,
                    method: "fixture/drop".into(),
                    params: serde_json::json!({})
                },
                None
            )
            .await,
        Err(RuntimeFailure::PipeEof)
    );
    process.finish().await.unwrap();
}
