use super::*;

fn make_supervisor(cli: PathBuf) -> Supervisor {
    Supervisor::new(cli, PathBuf::from(":memory:"))
}

/// Write a fake CLI script that echoes `stdout` and exits with `exit_code`.
/// Returns the path to the script (kept alive by `_dir`).
#[cfg(unix)]
fn fake_cli(dir: &tempfile::TempDir, stdout: &str, exit_code: i32) -> PathBuf {
    use std::io::Write;
    use std::os::unix::fs::PermissionsExt;
    let script = dir.path().join("fake-cli");
    let body = format!(
        "#!/bin/sh\necho '{}'\nexit {}\n",
        stdout.replace('\'', "'\\''"),
        exit_code
    );
    {
        let mut file = std::fs::File::create(&script).unwrap();
        file.write_all(body.as_bytes()).unwrap();
        // Force the write through before exec: a merely-closed fd can still
        // race spawn() as ETXTBSY on some filesystems under load.
        file.sync_all().unwrap();
    }
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

#[cfg(windows)]
fn fake_cli(dir: &tempfile::TempDir, stdout: &str, exit_code: i32) -> PathBuf {
    let script = dir.path().join("fake-cli.cmd");
    let body = format!("@echo off\r\necho {}\r\nexit /b {}\r\n", stdout, exit_code);
    std::fs::write(&script, body).unwrap();
    script
}

#[tokio::test]
async fn run_returns_report_on_success() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(
        &dir,
        r#"{"ok":true,"report":{"phase":"light","considered":3,"promoted":1,"dropped":0}}"#,
        0,
    );
    let sup = make_supervisor(cli);
    let report = sup.run(WorkerKind::DreamingLight).await.unwrap();
    assert_eq!(report.get("phase").and_then(|v| v.as_str()), Some("light"));
    assert_eq!(report.get("promoted").and_then(|v| v.as_u64()), Some(1));
}

#[tokio::test]
async fn process_split_contract_observer_success_failed_and_closed_delivery() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(&dir, r#"{"ok":true,"report":{"observed":true}}"#, 0);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let sup = make_supervisor(cli.clone());
    assert!(sup.observe_spawns(sender));
    assert_eq!(
        sup.run(WorkerKind::LibrarianOnce).await.unwrap()["observed"],
        true
    );
    let event = receiver.try_recv().unwrap();
    assert_eq!(event.executable, cli);
    assert!(event.pid > 0);
    assert!(receiver.try_recv().is_err());
    drop(event);
    drop(receiver);
    assert!(sup.run(WorkerKind::LibrarianOnce).await.is_ok());

    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let missing = make_supervisor(dir.path().join("missing"));
    missing.observe_spawns(sender);
    assert!(missing.run(WorkerKind::LibrarianOnce).await.is_err());
    assert!(matches!(
        receiver.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn process_split_contract_full_observer_and_default_preserve_maintenance() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(&dir, r#"{"ok":true,"report":{"observed":true}}"#, 0);
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let sup = make_supervisor(cli.clone());
    sup.observe_spawns(sender);
    for _ in 0..2 {
        assert!(sup.run(WorkerKind::LibrarianOnce).await.is_ok());
    }
    assert!(receiver.try_recv().is_ok());
    assert!(receiver.try_recv().is_err());
    assert!(make_supervisor(cli)
        .run(WorkerKind::LibrarianOnce)
        .await
        .is_ok());
}

/// The write descriptor is released only after the supervisor has been
/// observed refused, so the pass proves retry, never a lucky elapsed wait.
// macOS does not refuse a shebang script's exec while a write descriptor is
// open, so the fixture cannot arm there; Linux is where the race is real.
#[cfg(target_os = "linux")]
#[tokio::test]
async fn spawn_retries_after_deterministic_etxtbsy_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(
        &dir,
        r#"{"ok":true,"report":{"phase":"light","considered":3,"promoted":1,"dropped":0}}"#,
        0,
    );

    let holder = std::fs::OpenOptions::new().write(true).open(&cli).unwrap();
    let refused = std::process::Command::new(&cli)
        .output()
        .expect_err("fixture is only armed when the kernel refuses this exec");
    assert_eq!(
        refused.kind(),
        std::io::ErrorKind::ExecutableFileBusy,
        "fixture did not arm ETXTBSY: {refused}"
    );

    let sup = Arc::new(make_supervisor(cli));
    let mut refusals = sup.spawn_busy_retries();
    let runner = {
        let sup = Arc::clone(&sup);
        tokio::spawn(async move { sup.run(WorkerKind::DreamingLight).await })
    };

    // Bounded failure deadline around the event wait, never a readiness wait.
    tokio::time::timeout(Duration::from_secs(5), refusals.changed())
        .await
        .expect("supervisor never reported an ETXTBSY refusal")
        .expect("supervisor dropped the refusal channel");
    drop(holder);

    let outcome = runner.await.unwrap();
    let report = outcome.expect("spawn succeeds once the writer releases");
    assert_eq!(report.get("phase").and_then(|v| v.as_str()), Some("light"));
    assert_eq!(report.get("promoted").and_then(|v| v.as_u64()), Some(1));
    assert!(*refusals.borrow() >= 1);
}

#[tokio::test]
async fn spawn_does_not_retry_non_busy_errors() {
    let dir = tempfile::tempdir().unwrap();
    let sup = make_supervisor(dir.path().join("absent-cli"));
    let err = sup.run(WorkerKind::DreamingLight).await.unwrap_err();
    assert!(err.starts_with("spawn "), "got: {err}");
    assert_eq!(*sup.spawn_busy_retries().borrow(), 0);
}

#[tokio::test]
async fn run_returns_error_when_subprocess_fails() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(&dir, r#"{"ok":false,"error":"db locked"}"#, 1);
    let sup = make_supervisor(cli);
    let err = sup.run(WorkerKind::DreamingLight).await.unwrap_err();
    assert!(
        err.contains("exit 1") || err.contains("db locked"),
        "got: {}",
        err
    );
}

#[tokio::test]
async fn three_failures_open_circuit() {
    let dir = tempfile::tempdir().unwrap();
    let cli = fake_cli(&dir, "boom", 1);
    let sup = make_supervisor(cli);
    for _ in 0..3 {
        let _ = sup.run(WorkerKind::DreamingDeep).await;
    }
    let err = sup.run(WorkerKind::DreamingDeep).await.unwrap_err();
    assert!(
        err.contains("circuit open"),
        "expected circuit open, got: {}",
        err
    );

    sup.reset(WorkerKind::DreamingDeep);
    let err2 = sup.run(WorkerKind::DreamingDeep).await.unwrap_err();
    assert!(
        !err2.contains("circuit open"),
        "circuit should re-open after reset, got: {}",
        err2
    );
}

#[tokio::test]
async fn nonexistent_cli_path_fails_cleanly() {
    let sup = make_supervisor(PathBuf::from(
        "/definitely/not/a/real/path/codeterm-mem-worker-nope",
    ));
    let err = sup.run(WorkerKind::LibrarianOnce).await.unwrap_err();
    assert!(err.contains("spawn"), "expected spawn error, got: {}", err);
}

fn cli_filename() -> &'static str {
    if cfg!(windows) {
        "codeterm.exe"
    } else {
        "codeterm"
    }
}

fn touch(path: &std::path::Path) {
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p).unwrap();
    }
    std::fs::write(path, b"").unwrap();
}

#[test]
fn resolve_cli_path_env_override_wins() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(data_dir.join("bin")).unwrap();
    let installed = data_dir.join("bin").join(cli_filename());
    touch(&installed);

    let override_path = dir.path().join("custom").join(cli_filename());
    touch(&override_path);

    let resolved = resolve_cli_path_inner(&data_dir, Some(override_path.clone()), None, None);
    assert_eq!(
        resolved, override_path,
        "env override must win over installed"
    );
}

#[test]
fn resolve_cli_path_env_override_missing_falls_through() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    std::fs::create_dir_all(data_dir.join("bin")).unwrap();
    let installed = data_dir.join("bin").join(cli_filename());
    touch(&installed);

    let bogus = dir.path().join("does-not-exist").join(cli_filename());
    let resolved = resolve_cli_path_inner(&data_dir, Some(bogus), None, None);
    assert_eq!(
        resolved, installed,
        "missing override should fall through to installed"
    );
}

#[test]
fn resolve_cli_path_app_sibling_found() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let app = dir.path().join("MyApp.app").join("Contents").join("MacOS");
    std::fs::create_dir_all(&app).unwrap();
    let daemon = app.join("daemon-bin");
    touch(&daemon);
    let cli_sibling = app.join(cli_filename());
    touch(&cli_sibling);

    let resolved = resolve_cli_path_inner(&data_dir, None, Some(daemon), None);
    assert_eq!(resolved, cli_sibling);
}

#[test]
fn resolve_cli_path_target_sibling_found() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let target_dir = dir.path().join("target").join("debug");
    std::fs::create_dir_all(&target_dir).unwrap();
    let daemon = target_dir.join("some-daemon");
    touch(&daemon);
    let cli_sibling = target_dir.join(cli_filename());
    touch(&cli_sibling);

    let resolved = resolve_cli_path_inner(&data_dir, None, Some(daemon), None);
    assert_eq!(resolved, cli_sibling);
}

#[test]
fn resolve_cli_path_current_exe_is_cli_when_outside_target() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let install_dir = dir.path().join("Program Files").join("CodeTerm");
    std::fs::create_dir_all(&install_dir).unwrap();
    let exe = install_dir.join(cli_filename());
    touch(&exe);

    let resolved = resolve_cli_path_inner(&data_dir, None, Some(exe.clone()), None);
    assert_eq!(resolved, exe);
}

#[test]
fn resolve_cli_path_path_env_fallback() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let path_dir = dir.path().join("bin-on-path");
    std::fs::create_dir_all(&path_dir).unwrap();
    let on_path = path_dir.join(cli_filename());
    touch(&on_path);

    let path_env = std::env::join_paths([&path_dir]).unwrap();
    let resolved = resolve_cli_path_inner(&data_dir, None, None, Some(path_env));
    assert_eq!(resolved, on_path);
}

#[test]
fn resolve_cli_path_none_found_returns_installed() {
    let dir = tempfile::tempdir().unwrap();
    let data_dir = dir.path().join("data");
    let resolved = resolve_cli_path_inner(&data_dir, None, None, None);
    assert_eq!(resolved, data_dir.join("bin").join(cli_filename()));
}

/// Pin the wire contract with the CLI. If any of these strings change here,
/// `cli/src/main.rs::subcommand_contract_tests` must change in lockstep —
/// both halves are read together by the developer and grep-able.
#[test]
fn cli_subcommand_strings_are_pinned() {
    assert_eq!(MEM_PARENT_SUBCMD, "mem");
    assert_eq!(RUN_WORKER_SUBCMD, "run-worker");
    assert_eq!(KIND_DREAMING_LIGHT, "dreaming-light");
    assert_eq!(KIND_DREAMING_DEEP, "dreaming-deep");
    assert_eq!(KIND_DREAMING_NORMALIZE, "dreaming-normalize");
    assert_eq!(KIND_LIBRARIAN_ONCE, "librarian-once");
    assert_eq!(DB_PATH_FLAG, "--db-path");

    assert_eq!(WorkerKind::DreamingLight.as_arg(), KIND_DREAMING_LIGHT);
    assert_eq!(WorkerKind::DreamingDeep.as_arg(), KIND_DREAMING_DEEP);
    assert_eq!(
        WorkerKind::DreamingNormalize.as_arg(),
        KIND_DREAMING_NORMALIZE
    );
    assert_eq!(WorkerKind::LibrarianOnce.as_arg(), KIND_LIBRARIAN_ONCE);
}

/// Cross-crate sanity check: the CLI source still contains the literal
/// `#[clap(name = "run-worker")]` attribute and accepts every supervisor
/// kind. clap's `#[clap(name = ...)]` requires a literal so we can't share
/// a const directly — this string-search keeps the two halves wired
/// together at `cargo test --lib` time without needing the CLI binary to be
/// built first.
#[test]
fn cli_source_still_declares_run_worker() {
    let args_src = include_str!("../../../../../cli/src/args/mem.rs");
    assert!(
        args_src.contains(r#"#[clap(name = "run-worker")]"#),
        "cli/src/args/mem.rs no longer declares `#[clap(name = \"run-worker\")]` — \
             supervisor.rs spawns this exact subcommand. Fix one or the other."
    );
    let worker_src = include_str!("../../../../../cli/src/cmd/worker.rs");
    for kind in [
        KIND_DREAMING_LIGHT,
        KIND_DREAMING_DEEP,
        KIND_DREAMING_NORMALIZE,
        KIND_LIBRARIAN_ONCE,
    ] {
        assert!(
            worker_src.contains(&format!("\"{}\"", kind)),
            "cli/src/cmd/worker.rs no longer references kind {:?} — supervisor.rs spawns it.",
            kind
        );
    }
}

#[test]
fn backend_capability_selects_maintenance_route() {
    let sqlite = Supervisor::new(PathBuf::from("worker"), PathBuf::from("fixture.db"));
    assert_eq!(
        sqlite.maintenance_mode(),
        crate::backend::engines::MaintenanceMode::Subprocess {
            db_path: PathBuf::from("fixture.db")
        }
    );
    let manager = crate::MemManager::open_in_memory().unwrap();
    let memory = Supervisor::with_manager(PathBuf::from("worker"), manager);
    assert_eq!(
        memory.maintenance_mode(),
        crate::backend::engines::MaintenanceMode::InProcess
    );
}
