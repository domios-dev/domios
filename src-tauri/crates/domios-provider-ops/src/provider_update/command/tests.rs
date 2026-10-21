//! Native executors and confined resolution must preserve completion and failure evidence.
//! These controls reproduce parent-only success and discarded stream/termination errors.
//! Socket gates, OS exit receipts, bounded tails and child receipts supply the oracles.

use super::*;

#[test]
fn provider_update_output_is_drained_with_a_bounded_tail() {
    let mut bytes = vec![b'x'; 65536];
    bytes.extend(b"opaque-final-output");
    let mut stream = capture(std::io::Cursor::new(bytes)).unwrap();
    stream
        .receive(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .unwrap();
    let output = captured(Some(&stream));
    assert_eq!(output.len(), 32768);
    assert!(output.ends_with("opaque-final-output"));
}

#[test]
fn provider_update_command_rejects_shell_input() {
    assert!(update_argv("updater update; injected").is_err());
    assert!(update_argv("updater $(injected)").is_err());
    assert!(update_argv("").is_err());
}

#[test]
fn provider_update_command_real_success_waits_for_normal_children() {
    let mut fixture = process_fixture::Fixture::start("success");
    fixture.release("descendant");
    fixture.release("parent");
    assert!(fixture.finish().is_ok());
}

#[test]
fn provider_update_command_parent_zero_with_closed_stream_live_child_is_unconfirmed() {
    let mut fixture = process_fixture::Fixture::start("closed");
    fixture.release("parent");
    fixture.parent_reaped();
    fixture.continue_parent();
    let failure = fixture
        .result
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .expect_err("parent exit zero is insufficient");
    assert!(failure.reason.contains("CaptureDeadline") || failure.reason.contains("Boundary"));
    #[cfg(unix)]
    assert!(!failure.child_stopped);
}

#[test]
fn provider_update_command_parent_zero_with_inherited_stream_live_child_is_unconfirmed() {
    let mut fixture = process_fixture::Fixture::start("inherited");
    fixture.release("parent");
    fixture.parent_reaped();
    fixture.continue_parent();
    let failure = fixture
        .result
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .expect_err("remaining child was killed to manufacture success");
    assert!(failure.reason.contains("CaptureDeadline") || failure.reason.contains("Boundary"));
    #[cfg(unix)]
    assert!(!failure.child_stopped);
}

#[test]
fn provider_update_command_real_nonzero_retains_tail_and_never_restarts() {
    let mut fixture = process_fixture::Fixture::start("nonzero");
    fixture.release("descendant");
    fixture.release("parent");
    let failure = fixture.finish().err().unwrap();
    assert!(failure.child_stopped);
    assert!(failure.reason.contains("fixture-error-tail"));
    assert!(!failure.reason.contains(&"x".repeat(32769)));
    let mut state = super::super::state::Coordinator::default();
    state.begin("fixture", "1", "2");
    let reason = failure.reason;
    state.finish("fixture:2", Err(reason.clone()));
    assert_eq!(
        state.runs["fixture:2"].outcome,
        super::super::state::RunOutcome::Failed(reason)
    );
}

#[test]
fn provider_update_command_deadline_is_failure_after_native_cleanup() {
    let mut fixture = process_fixture::Fixture::start("deadline");
    fixture.expire();
    let failure = fixture.finish().err().unwrap();
    assert!(failure.reason.contains("ExecutionDeadline"));
    assert!(failure.child_stopped);
}

#[test]
fn provider_update_command_termination_error_blocks_further_runs() {
    let mut fixture = process_fixture::Fixture::start("termination");
    fixture.expire();
    let failure = fixture
        .result
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap()
        .err()
        .unwrap();
    assert!(failure.reason.contains("injected termination API error"));
    assert!(!failure.child_stopped);
    let mut state = super::super::state::Coordinator::default();
    state.begin("fixture", "1", "2");
    state.finish("fixture:2", Err(failure.reason));
    state.block_unstopped("fixture:2");
    assert_eq!(
        state.begin("fixture", "1", "3"),
        super::super::state::Admission::ProviderBusy
    );
}

struct FaultyRead {
    sent: bool,
}
impl std::io::Read for FaultyRead {
    fn read(&mut self, chunk: &mut [u8]) -> std::io::Result<usize> {
        if self.sent {
            return Err(std::io::Error::other("opaque-read-error"));
        }
        self.sent = true;
        chunk[..4].copy_from_slice(b"tail");
        Ok(4)
    }
}

#[test]
fn provider_update_command_capture_read_error_retains_tail_and_is_not_eof() {
    let mut stream = capture(FaultyRead { sent: false }).unwrap();
    let error = stream
        .receive(std::time::Instant::now() + std::time::Duration::from_secs(1))
        .unwrap_err();
    assert!(matches!(error.stage, FailureStage::CaptureRead));
    assert_eq!(error.detail, "opaque-read-error");
    assert_eq!(captured(Some(&stream)), "tail");
    assert!(stream.resources_complete());
    assert!(!stream.clean_eof());
}

#[test]
fn provider_update_command_capture_disconnect_and_expiry_are_distinct_failures() {
    let (tx, receiver) = std::sync::mpsc::channel();
    let mut stream = Capture {
        receiver,
        tail: Default::default(),
        ended: None,
    };
    let error = stream.receive(std::time::Instant::now()).unwrap_err();
    assert!(matches!(error.stage, FailureStage::CaptureDeadline));
    drop(tx);
    let error = stream.receive(std::time::Instant::now()).unwrap_err();
    assert!(matches!(error.stage, FailureStage::CaptureDisconnect));
    assert!(!stream.resources_complete());
}

#[test]
fn provider_update_command_capture_and_setup_faults_invalidate_real_parent_success() {
    for kind in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let mut invocation = receipt_invocation(root.path());
        invocation.controls.capture_setup_error = kind == 0;
        invocation.controls.capture_read_error = kind == 1;
        invocation.controls.capture_disconnect = kind == 2;
        let failure = run_invocation(invocation).expect_err("capture failure returned success");
        assert!(failure.reason.contains(match kind {
            0 => "CaptureSetup",
            1 => "CaptureRead",
            _ => "CaptureDisconnect",
        }));
    }
}

fn receipt_invocation(root: &Path) -> Invocation {
    let mut invocation = process_fixture::invocation("receipt", "unused");
    invocation.cwd = Some(root.to_path_buf());
    if let EnvironmentPolicy::Explicit(environment) = &mut invocation.environment {
        environment.push((
            "COURIER_FIXTURE_RECEIPT".into(),
            root.join("receipt.json").into_os_string(),
        ));
        environment.push((
            "COURIER_FIXTURE_TOKEN".into(),
            "opaque-fixture-token".into(),
        ));
    }
    invocation
}

fn confined_policy(path: &Path) -> ResolutionPolicy {
    ResolutionPolicy::Confined(codeterm_plugin::js_runtime::BinSearchEnvironment {
        inherited_path: Some(path.as_os_str().to_owned()),
        machine_path: codeterm_plugin::js_runtime::EnvironmentValue::Missing,
        user_path: codeterm_plugin::js_runtime::EnvironmentValue::Missing,
        pathext: Some(".EXE;.CMD".into()),
    })
}

#[test]
fn provider_update_command_confined_resolution_executes_exact_argv_cwd_and_environment() {
    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("selected with spaces");
    let decoy = root.path().join("ambient rescue decoy");
    std::fs::create_dir(&selected).unwrap();
    std::fs::create_dir(&decoy).unwrap();
    let name = if cfg!(windows) {
        "courier-updater.exe"
    } else {
        "courier-updater"
    };
    let executable = selected.join(name);
    #[cfg(unix)]
    std::os::unix::fs::symlink(std::env::current_exe().unwrap(), &executable).unwrap();
    #[cfg(windows)]
    std::fs::copy(std::env::current_exe().unwrap(), &executable).unwrap();
    let decoy_path = decoy.join(name);
    std::fs::write(&decoy_path, "opaque-untouched-decoy").unwrap();
    let (resolved, _) =
        update_argv_with_policy("courier-updater", &confined_policy(&selected)).unwrap();
    assert_eq!(resolved, executable);
    let mut invocation = receipt_invocation(root.path());
    invocation.executable = resolved;
    invocation
        .args
        .extend(["--skip".into(), "opaque argument with spaces".into()]);
    let expected_args = invocation
        .args
        .iter()
        .map(|arg| arg.to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    if let Err(failure) = run_invocation(invocation) {
        panic!(
            "{}; child_stopped={}",
            failure.reason, failure.child_stopped
        );
    }
    let receipt: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("receipt.json")).unwrap()).unwrap();
    assert_eq!(
        std::fs::canonicalize(receipt["executable"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(&executable).unwrap()
    );
    assert_eq!(
        std::fs::canonicalize(receipt["cwd"].as_str().unwrap()).unwrap(),
        std::fs::canonicalize(root.path()).unwrap()
    );
    assert_eq!(receipt["args"], serde_json::json!(expected_args));
    assert_eq!(
        receipt["environment"]["COURIER_FIXTURE_TOKEN"],
        "opaque-fixture-token"
    );
    for forbidden in [
        "CODETERM_API_TOKEN",
        "CODETERM_API_URL",
        "CODETERM_INSTANCE",
        "CODETERM_TAB_ID",
        "HOME",
        "USERPROFILE",
        "CODEX_HOME",
        "CLAUDE_CONFIG_DIR",
        "OPENAI_API_KEY",
    ] {
        assert!(
            receipt["environment"].get(forbidden).is_none(),
            "ambient selector leaked: {forbidden}"
        );
    }
    assert_eq!(
        std::fs::read_to_string(decoy_path).unwrap(),
        "opaque-untouched-decoy"
    );
    let empty = root.path().join("empty resolution roots");
    std::fs::create_dir(&empty).unwrap();
    assert!(update_argv_with_policy("courier-updater", &confined_policy(&empty)).is_err());
}

#[test]
fn provider_update_command_node_and_shim_share_the_confined_resolution_policy() {
    let root = tempfile::tempdir().unwrap();
    let selected = root.path().join("node with spaces");
    std::fs::create_dir(&selected).unwrap();
    let shim = selected.join("updater.cmd");
    std::fs::write(&shim, "\"%_prog%\" \"%dp0%\\script with spaces.js\" %*\n").unwrap();
    let policy = confined_policy(&selected);
    assert!(windows_npm_shim_argv(&shim, &["update"], &policy).is_err());
    let node = selected.join(if cfg!(windows) { "node.exe" } else { "node" });
    std::fs::copy(std::env::current_exe().unwrap(), &node).unwrap();
    let (resolved, argv) = windows_npm_shim_argv(&shim, &["update"], &policy).unwrap();
    assert_eq!(resolved, node);
    assert_eq!(
        argv,
        vec![
            selected.join("script with spaces.js").into_os_string(),
            "update".into()
        ]
    );
}

#[cfg(windows)]
#[test]
fn provider_update_command_job_waits_for_child_after_parent_and_closed_streams() {
    let mut fixture = process_fixture::Fixture::start("windows-wait");
    fixture.release("parent");
    fixture.parent_reaped();
    fixture.release("descendant");
    fixture.continue_parent();
    assert!(fixture.finish().is_ok());
}

#[test]
fn provider_update_command_pre_spawn_refusal_has_no_running_child() {
    let failure = run_update_command("updater update; injected")
        .err()
        .unwrap();
    assert!(failure.child_stopped);
    assert!(failure.reason.starts_with("Resolve:"));
}
