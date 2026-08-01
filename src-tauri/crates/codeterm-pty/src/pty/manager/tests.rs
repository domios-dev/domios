use super::*;
#[cfg(target_os = "windows")]
use crate::pty::windows_test_support::{
    conpty_host_children_of_current_process, exclusive_conpty_test, legacy_conhost_children,
    new_conpty_hosts, wait_for_processes_gone,
};
use std::io::Write;
use std::sync::{mpsc, Arc, Mutex};

struct DelayedFirstWrite {
    data: Arc<Mutex<Vec<u8>>>,
    first_entered: mpsc::SyncSender<()>,
    release_first: mpsc::Receiver<()>,
    landed: mpsc::Sender<Vec<u8>>,
    first: bool,
}

impl Write for DelayedFirstWrite {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if self.first {
            self.first = false;
            self.first_entered.send(()).unwrap();
            self.release_first.recv().unwrap();
        }
        self.data.lock().unwrap().extend_from_slice(buf);
        self.landed.send(buf.to_vec()).unwrap();
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn queued_unguarded_write_is_captured_when_ack_wait_times_out() {
    static NEXT_CAPTURE: AtomicU64 = AtomicU64::new(1);

    let data = Arc::new(Mutex::new(Vec::new()));
    let (first_entered_tx, first_entered_rx) = mpsc::sync_channel(0);
    let (release_first_tx, release_first_rx) = mpsc::sync_channel(0);
    let (landed_tx, landed_rx) = mpsc::channel();
    let queue = WriteQueue::new(Box::new(DelayedFirstWrite {
        data: data.clone(),
        first_entered: first_entered_tx,
        release_first: release_first_rx,
        landed: landed_tx,
        first: true,
    }));
    queue.send(b"blocker".to_vec()).unwrap();
    first_entered_rx
        .recv_timeout(std::time::Duration::from_secs(1))
        .unwrap();

    let capture_id = NEXT_CAPTURE.fetch_add(1, Ordering::Relaxed);
    let pty_id = format!("ack-timeout-capture-{}-{capture_id}", std::process::id());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("queued-timeout.cast");
    let registry = crate::pty::capture::CaptureRegistry::global();
    registry
        .start(&pty_id, path.clone(), 80, 24, 0, Vec::new())
        .unwrap();

    let payload = b"queued-payload";
    let acknowledgment = queue.enqueue_with_ack(payload.to_vec()).unwrap();
    let result = PtyWriteAcknowledgement::unguarded(7, &pty_id, payload, acknowledgment)
        .wait(std::time::Duration::ZERO);
    assert!(
        matches!(
            result.as_ref(),
            Err(error) if super::super::write_queue::is_write_ack_timeout(error)
        ),
        "the queued write must remain uncertain while the writer is blocked: {result:?}"
    );

    release_first_tx.send(()).unwrap();
    assert_eq!(
        landed_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap(),
        b"blocker"
    );
    assert_eq!(
        landed_rx
            .recv_timeout(std::time::Duration::from_secs(1))
            .unwrap(),
        payload
    );
    assert_eq!(&*data.lock().unwrap(), b"blockerqueued-payload");

    registry.stop(&pty_id).unwrap();
    let capture = std::fs::read_to_string(path).unwrap();
    assert!(capture.contains("\"source\":\"pty.write\""));
    assert!(capture.contains("\"text\":\"queued-payload\""));
}

/// Load-bearing contract for the reader-race guard in read_pty_core: once a
/// backend channel is claimed, has_backend_reader flips true and the rx side
/// exists — frontends must consume the channel instead of cloning a second
/// raw reader (two readers on one master fd split the byte stream).
#[test]
fn backend_channel_claim_is_visible_and_idempotent() {
    #[cfg(target_os = "windows")]
    let _guard = exclusive_conpty_test();
    let mut manager = PtyManager::new();
    let (pty_id, session) = manager
        .spawn(None, None, 80, 24, HashMap::new())
        .expect("spawn default shell");

    assert!(
        !manager.has_backend_reader(&pty_id),
        "fresh session must not report a backend reader"
    );

    let tx1 = manager
        .setup_backend_channel(&pty_id)
        .expect("first claim succeeds");
    assert!(
        manager.has_backend_reader(&pty_id),
        "claim must be visible immediately (read_pty guard keys on this)"
    );
    assert!(
        session.backend_reader_rx.lock().unwrap().is_some(),
        "rx side must exist for the frontend to consume"
    );

    let tx2 = manager
        .setup_backend_channel(&pty_id)
        .expect("second claim returns the existing channel");
    assert!(
        tx1.same_channel(&tx2),
        "re-claim must NOT create a new channel (would orphan the rx)"
    );

    manager.kill(&pty_id).expect("cleanup");
}

#[test]
fn kill_terminates_child_and_removes_session() {
    #[cfg(target_os = "windows")]
    let _guard = exclusive_conpty_test();
    let mut manager = PtyManager::new();
    let (pty_id, session) = manager
        .spawn(None, None, 80, 24, HashMap::new())
        .expect("spawn default shell");

    let pid = session.child.lock().expect("child mutex").process_id();
    assert!(pid.is_some(), "spawned child should expose a pid");

    manager.kill(&pty_id).expect("kill should succeed");

    assert!(
        manager.get_session(&pty_id).is_none(),
        "session should be gone from manager after kill"
    );

    session
        .teardown
        .wait_for_completion(std::time::Instant::now() + std::time::Duration::from_secs(5))
        .expect("child and backend reader must finish after PtyManager::kill");
}

/// Regression guard for the CREATE_NO_WINDOW ConPTY bug: spawning a child
/// with CREATE_NO_WINDOW detaches it from the pseudoconsole, so the conout
/// reader gets zero bytes and every terminal renders blank. A bare cmd.exe
/// must produce output through the PTY reader within a few seconds.
#[cfg(target_os = "windows")]
#[test]
fn conpty_shell_produces_output() {
    let _guard = exclusive_conpty_test();
    use std::io::Read;
    use std::sync::mpsc;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let _child = pair
        .slave
        .spawn_command(CommandBuilder::new("cmd.exe"))
        .expect("spawn cmd.exe");
    drop(pair.slave);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).unwrap_or(0);
        buf.truncate(n);
        let _ = tx.send(buf);
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(8))
        .unwrap_or_default();
    assert!(
        !got.is_empty(),
        "ConPTY cmd.exe produced no output within 8s — CREATE_NO_WINDOW likely re-added to the pseudoconsole spawn"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn conpty_cmd_produces_prompt_beyond_control_bytes() {
    let _guard = exclusive_conpty_test();
    use std::io::Read;
    use std::sync::mpsc;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let _child = pair
        .slave
        .spawn_command(CommandBuilder::new("cmd.exe"))
        .expect("spawn cmd.exe");
    drop(pair.slave);
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut got = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(8);
        while std::time::Instant::now() < deadline {
            let mut buf = vec![0u8; 4096];
            let n = reader.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.truncate(n);
            got.extend_from_slice(&buf);
            if String::from_utf8_lossy(&got).contains('>') {
                break;
            }
        }
        let _ = tx.send(got);
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(10))
        .unwrap_or_default();
    let text = String::from_utf8_lossy(&got);
    assert!(
        text.contains('>') && got.len() > 4,
        "cmd.exe never produced a prompt beyond control bytes; bytes={:?}, text={:?}",
        got,
        text
    );
}

#[cfg(target_os = "windows")]
#[test]
fn conpty_pwsh_prompt_produces_output_beyond_cursor_query() {
    let _guard = exclusive_conpty_test();
    use std::io::{Read, Write};
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    let shell = detect_default_shell();
    assert!(
        shell.to_lowercase().contains("pwsh") || shell.to_lowercase().contains("powershell"),
        "expected Windows PowerShell default shell, got {shell}"
    );

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let mut writer = pair.master.take_writer().expect("take writer");
    let mut cmd = CommandBuilder::new(&shell);
    crate::pty::shell_integration::apply_env(&mut cmd, &shell);
    let _child = pair.slave.spawn_command(cmd).expect("spawn pwsh");
    drop(pair.slave);

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(8);
        while Instant::now() < deadline {
            let mut buf = vec![0u8; 4096];
            let n = reader.read(&mut buf).unwrap_or(0);
            if n == 0 {
                break;
            }
            buf.truncate(n);
            if tx.send(buf).is_err() {
                break;
            }
        }
    });

    let mut got = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let chunk = rx.recv_timeout(remaining).unwrap_or_default();
        if chunk.is_empty() {
            break;
        }
        if chunk.windows(4).any(|w| w == b"\x1b[6n") {
            let _ = writer.write_all(b"\x1b[1;1R");
            let _ = writer.flush();
        }
        got.extend_from_slice(&chunk);
        let text = String::from_utf8_lossy(&got);
        if text.contains("PS ") && text.contains('>') {
            break;
        }
    }
    assert!(
        got.len() > 4,
        "pwsh produced only terminal capability bytes: {:?}",
        got
    );
    let text = String::from_utf8_lossy(&got);
    assert!(
        text.contains("PS ") && text.contains('>'),
        "pwsh never produced a prompt beyond cursor query; bytes={:?}, text={:?}",
        got,
        text
    );
}

#[cfg(target_os = "windows")]
#[test]
fn conpty_shell_does_not_spawn_legacy_child_conhost() {
    let _guard = exclusive_conpty_test();
    use std::io::Read;
    use std::sync::mpsc;
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows: 30,
            cols: 100,
            pixel_width: 0,
            pixel_height: 0,
        })
        .expect("openpty");
    let mut reader = pair.master.try_clone_reader().expect("clone reader");
    let child = pair
        .slave
        .spawn_command(CommandBuilder::new("cmd.exe"))
        .expect("spawn cmd.exe");
    drop(pair.slave);

    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = vec![0u8; 4096];
        let n = reader.read(&mut buf).unwrap_or(0);
        buf.truncate(n);
        let _ = tx.send(buf);
    });
    let got = rx
        .recv_timeout(std::time::Duration::from_secs(8))
        .unwrap_or_default();
    assert!(!got.is_empty(), "ConPTY child produced no output");

    let child_pid = child.process_id().expect("child pid");
    let legacy = legacy_conhost_children(child_pid);
    assert!(
        legacy.is_empty(),
        "ConPTY child {} spawned legacy conhost children {:?}; only the headless ConPTY host should exist",
        child_pid,
        legacy
    );
}

#[cfg(target_os = "windows")]
#[test]
fn kill_reaps_conpty_host_before_next_spawn() {
    let _guard = exclusive_conpty_test();
    let mut manager = PtyManager::new();
    let before = conpty_host_children_of_current_process();
    let (old_pty, old_session) = manager
        .spawn(None, None, 80, 24, HashMap::new())
        .expect("spawn old pty");
    let old_child = old_session
        .child
        .lock()
        .expect("old child mutex")
        .process_id()
        .expect("old child pid");
    let old_hosts = new_conpty_hosts(&before);
    assert_eq!(
        old_hosts.len(),
        1,
        "old PTY should create exactly one ConPTY host; got {old_hosts:?}"
    );

    manager
        .begin_kill(&old_pty)
        .expect("begin old pty kill")
        .wait_bounded(std::time::Duration::from_secs(5))
        .expect("old PTY process tree must reap before retry");
    assert!(
        wait_for_processes_gone(
            old_hosts
                .iter()
                .copied()
                .chain(std::iter::once(old_child))
                .collect(),
            std::time::Duration::from_secs(5),
        ),
        "old child {old_child} and ConPTY hosts {old_hosts:?} must exit after kill"
    );

    let after_old = conpty_host_children_of_current_process();
    let (new_pty, new_session) = manager
        .spawn(None, None, 80, 24, HashMap::new())
        .expect("spawn new pty");
    let new_hosts = new_conpty_hosts(&after_old);
    assert_eq!(
        new_hosts.len(),
        1,
        "new PTY should own exactly one ConPTY host; got {new_hosts:?}"
    );
    let new_child = new_session
        .child
        .lock()
        .expect("new child mutex")
        .process_id()
        .expect("new child pid");
    manager
        .begin_kill(&new_pty)
        .expect("begin new pty kill")
        .wait_bounded(std::time::Duration::from_secs(5))
        .expect("new PTY process tree must reap");
    assert!(
        wait_for_processes_gone(
            new_hosts
                .iter()
                .copied()
                .chain(std::iter::once(new_child))
                .collect(),
            std::time::Duration::from_secs(5),
        ),
        "new child {new_child} and ConPTY hosts {new_hosts:?} must exit after kill"
    );
}

#[cfg(target_os = "windows")]
#[test]
fn shutdown_reaps_every_owned_pty_and_conpty_host() {
    let _guard = exclusive_conpty_test();
    let mut manager = PtyManager::new();
    let before = conpty_host_children_of_current_process();
    let mut child_pids = Vec::new();
    let mut first_session = None;
    for _ in 0..2 {
        let (_, session) = manager
            .spawn(None, None, 80, 24, HashMap::new())
            .expect("spawn owned pty");
        first_session.get_or_insert_with(|| session.clone());
        child_pids.push(
            session
                .child
                .lock()
                .expect("child mutex")
                .process_id()
                .expect("child pid"),
        );
    }
    let hosts = new_conpty_hosts(&before);
    assert_eq!(hosts.len(), 2, "each owned PTY must have a tracked host");
    let first_session = first_session.expect("first session");
    first_session
        .child
        .lock()
        .expect("first child mutex")
        .kill()
        .expect("pre-shutdown process-tree termination");

    let reapers = manager.begin_shutdown().expect("begin PTY shutdown");
    assert_eq!(reapers.len(), 2);
    assert!(manager.sessions.is_empty());
    let spawn_error = match manager.spawn(None, None, 80, 24, HashMap::new()) {
        Ok(_) => panic!("shutdown manager accepted a new PTY"),
        Err(error) => error,
    };
    assert!(spawn_error.contains("quiesced"));
    for reaper in reapers {
        reaper
            .wait_bounded(std::time::Duration::from_secs(5))
            .expect("owned PTY must reap");
    }

    assert!(
        wait_for_processes_gone(
            hosts.into_iter().chain(child_pids).collect(),
            std::time::Duration::from_secs(5),
        ),
        "owned PTY children and ConPTY hosts must all exit"
    );
}

#[test]
fn kill_returns_err_for_unknown_pty() {
    let mut manager = PtyManager::new();
    let result = manager.kill("nonexistent-pty-id");
    assert!(result.is_err(), "kill on unknown id must return Err");
}

/// Interactive tabs must never look like a CI job: an inherited CI=true makes
/// cursor-agent (and other is-ci-aware TUIs) go non-interactive and hide their
/// input bar even with a real TTY. Guard against the strip list regressing.
#[test]
fn stripped_pty_env_drops_ci_markers() {
    assert!(STRIPPED_PTY_ENV.contains(&"CI"), "CI must be stripped");
    assert!(
        STRIPPED_PTY_ENV.contains(&"CONTINUOUS_INTEGRATION"),
        "CONTINUOUS_INTEGRATION must be stripped"
    );
    assert!(STRIPPED_PTY_ENV.contains(&"CLAUDECODE"));
    assert!(STRIPPED_PTY_ENV.contains(&"NO_COLOR"));
}

#[test]
fn pty_env_drops_inherited_orchestration_authority() {
    let inherited = [
        "CODETERM_PLAN_ID",
        "CODETERM_TASK_ID",
        "CODETERM_LANE_BASE",
        "CODETERM_ROLE",
        "CODETERM_PARENT_TAB_ID",
        "CODETERM_WORKER_ID",
    ];
    let mut cmd = CommandBuilder::new("probe");
    for key in inherited {
        cmd.env(key, "foreign");
    }

    apply_pty_env(&mut cmd, &HashMap::new());

    for key in inherited {
        assert_eq!(cmd.get_env(key), None, "{key} leaked into roleless PTY");
    }
}

#[test]
fn pty_env_preserves_explicit_assigned_worker_authority() {
    let mut cmd = CommandBuilder::new("worker");
    cmd.env("CODETERM_PLAN_ID", "foreign-plan");
    cmd.env("CODETERM_TASK_ID", "foreign-task");
    cmd.env("CODETERM_ROLE", "Orchestrator");
    let assigned = HashMap::from([
        ("CODETERM_PLAN_ID".to_string(), "assigned-plan".to_string()),
        ("CODETERM_TASK_ID".to_string(), "assigned-task".to_string()),
        (
            "CODETERM_LANE_BASE".to_string(),
            "assigned-base".to_string(),
        ),
        ("CODETERM_ROLE".to_string(), "Worker".to_string()),
        (
            "CODETERM_PARENT_TAB_ID".to_string(),
            "assigned-parent".to_string(),
        ),
        (
            "CODETERM_WORKER_ID".to_string(),
            "assigned-worker".to_string(),
        ),
    ]);

    apply_pty_env(&mut cmd, &assigned);

    for (key, value) in assigned {
        assert_eq!(
            cmd.get_env(&key),
            Some(std::ffi::OsStr::new(&value)),
            "{key} did not use its explicit assignment"
        );
    }
}

#[test]
fn pty_env_prefers_the_configured_codeterm_cli() {
    let cli_path = if cfg!(windows) {
        r"C:\dev\bin\codeterm.exe"
    } else {
        "/tmp/dev/bin/codeterm"
    };
    let other_path = if cfg!(windows) {
        r"C:\prod\bin"
    } else {
        "/tmp/prod/bin"
    };
    let expected = std::path::Path::new(cli_path).parent().unwrap();
    let inherited = std::env::join_paths([other_path]).unwrap();
    let assigned = HashMap::from([
        ("CODETERM_CLI_PATH".to_string(), cli_path.to_string()),
        ("PATH".to_string(), inherited.to_string_lossy().into_owned()),
    ]);
    let mut cmd = CommandBuilder::new("probe");

    apply_pty_env(&mut cmd, &assigned);

    let paths: Vec<_> = std::env::split_paths(cmd.get_env("PATH").unwrap()).collect();
    assert_eq!(paths.first().map(std::path::PathBuf::as_path), Some(expected));
}

/// `launch_agent_in_background` relies on `write` returning `Err("PTY not
/// found: <id>")` so the success log is gated and the misleading "agent
/// launched" line never fires when the PTY lookup races and misses.
#[test]
fn write_returns_err_for_unknown_pty() {
    let manager = PtyManager::new();
    let err = manager
        .write("missing-pty-id", b"hello")
        .expect_err("write on unknown id must return Err");
    assert!(
        err.contains("PTY not found"),
        "error must signal missing PTY (got {:?})",
        err
    );
    assert!(
        err.contains("missing-pty-id"),
        "error must include the offending id (got {:?})",
        err
    );
}
