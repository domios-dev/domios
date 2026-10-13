use super::*;
use codeterm_plugin::agent_runtime::{AgentRuntimeCommand, AgentRuntimeProtocol, RpcCall};
fn capability() -> AgentRuntimeCapability {
    AgentRuntimeCapability {
        runtime: AgentRuntimeProtocol::StdioJsonrpc,
        command: AgentRuntimeCommand {
            binary: "node".into(),
            args: vec![format!(
                "{}/../tests/fixtures/agent-runtime/fake-peer.mjs",
                env!("CARGO_MANIFEST_DIR")
            )],
        },
        inherited_environment_exclusions: Vec::new(),
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
async fn child_exit_reports_status_and_stderr_and_pipe_drop_is_not_success() {
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
