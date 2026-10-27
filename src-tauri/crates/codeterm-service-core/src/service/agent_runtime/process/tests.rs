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
                params: serde_json::json!({}),
            },
            None,
        )
        .await
        .unwrap()
        .unwrap();
    let pids: Vec<u32> = serde_json::from_value(result["pids"].clone()).unwrap();
    assert_eq!(pids.len(), 2);
    let members = process.boundary.as_ref().unwrap().snapshot().unwrap();
    for pid in &pids {
        let member = members.iter().find(|member| member.pid == *pid).unwrap();
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
