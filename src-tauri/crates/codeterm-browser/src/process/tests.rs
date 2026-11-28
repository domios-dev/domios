use super::*;

const FAKE_ENDPOINT: &str = "ws://127.0.0.1:9/devtools/browser/fake";

#[cfg(windows)]
fn fake_browser(stderr_line: &str, hold: bool) -> (&'static str, Vec<String>) {
    let hold = if hold {
        "; [Console]::In.ReadLine() | Out-Null"
    } else {
        ""
    };
    (
        "powershell",
        vec![
            "-NoProfile".into(),
            "-NonInteractive".into(),
            "-Command".into(),
            format!("[Console]::Error.WriteLine('{stderr_line}'){hold}"),
        ],
    )
}

#[cfg(unix)]
fn fake_browser(stderr_line: &str, hold: bool) -> (&'static str, Vec<String>) {
    let hold = if hold { "; read line" } else { "" };
    (
        "sh",
        vec!["-c".into(), format!("echo '{stderr_line}' >&2{hold}")],
    )
}

#[tokio::test]
async fn browser_process_ready_from_stderr_then_killed_on_request() {
    let (binary, args) = fake_browser(&format!("DevTools listening on {FAKE_ENDPOINT}"), true);
    let process =
        BrowserProcess::spawn_with_stdin(Path::new(binary), &args, READY_DEADLINE, Stdio::piped())
            .await
            .unwrap();
    assert_eq!(process.endpoint(), FAKE_ENDPOINT);
    assert_eq!(process.exit_status(), None);
    let outcome = process.close_with(async {}, Duration::ZERO).await;
    assert!(
        matches!(outcome, CloseOutcome::Killed(Some(_))),
        "{outcome:?}"
    );
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn browser_process_descendant_pipe_closes_when_the_root_exits() {
    use std::os::unix::ffi::OsStrExt;
    use tokio::io::AsyncWriteExt;
    use tokio::net::unix::pipe::OpenOptions;

    let root = tempfile::tempdir().unwrap();
    let control = root.path().join("exit");
    let witness = root.path().join("descendant");
    for path in [&control, &witness] {
        let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }
    let mut signal = OpenOptions::new()
        .read_write(true)
        .open_sender(&control)
        .unwrap();
    let mut witness_pipe = OpenOptions::new().open_receiver(&witness).unwrap();
    let args = vec![
        "-c".into(),
        format!(
            "exec 4<&0; sh -c 'exec 3>\"$1\"; printf x >&3; printf \"%s\\n\" \"$2\" >&2; read line <&4' \
             sh \"$1\" 'DevTools listening on {FAKE_ENDPOINT}' & read line <\"$2\""
        ),
        "sh".into(),
        witness.to_string_lossy().into_owned(),
        control.to_string_lossy().into_owned(),
    ];
    let process =
        BrowserProcess::spawn_with_stdin(Path::new("sh"), &args, READY_DEADLINE, Stdio::piped())
            .await
            .unwrap();
    let mut byte = [0];
    tokio::time::timeout(KILL_DEADLINE, witness_pipe.read_exact(&mut byte))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(byte, [b'x']);
    signal.write_all(b"exit\n").await.unwrap();
    signal.flush().await.unwrap();
    assert!(process
        .wait_exit_until(Instant::now() + KILL_DEADLINE)
        .await
        .is_some());
    assert_eq!(
        tokio::time::timeout(KILL_DEADLINE, witness_pipe.read(&mut byte))
            .await
            .unwrap()
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn browser_process_exit_before_devtools_is_an_error() {
    let (binary, args) = fake_browser("Opening in existing browser session.", false);
    let error = BrowserProcess::spawn(Path::new(binary), &args, READY_DEADLINE)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error.cause,
            SpawnFailure::ExitedBeforeDevTools {
                exit: ProcessExit { code: Some(0) },
                ref output,
            } if output == "Opening in existing browser session."
        ),
        "{error}"
    );
}

#[cfg(windows)]
#[tokio::test]
async fn browser_process_silent_zero_exit_before_devtools_has_a_typed_cause() {
    let args = ["-NoProfile", "-NonInteractive", "-Command", "exit 0"].map(str::to_string);
    let error = BrowserProcess::spawn(Path::new("powershell"), &args, READY_DEADLINE)
        .await
        .unwrap_err();
    assert!(
        matches!(
            error.cause,
            SpawnFailure::ExitedBeforeDevTools {
                exit: ProcessExit { code: Some(0) },
                ref output,
            } if output.is_empty()
        ),
        "{error}"
    );
    assert!(!error.process_may_be_running);
    assert_eq!(
        error.to_string(),
        "browser_exited_before_devtools: browser exited before DevTools was ready (exit code 0); no startup output was captured"
    );
}

#[tokio::test]
async fn browser_process_foreign_profile_handoff_on_stdout_keeps_diagnostic() {
    let (binary, args) = fake_browser("Opening in existing browser session.", false);
    let args: Vec<String> = args
        .into_iter()
        .map(|arg| {
            arg.replace("[Console]::Error", "[Console]")
                .replace(" >&2", "")
        })
        .collect();
    let error = BrowserProcess::spawn(Path::new(binary), &args, READY_DEADLINE)
        .await
        .unwrap_err()
        .to_string();
    let dir = Path::new("/profiles/foreign");
    let error = super::super::profile::foreign_profile_error(dir, &error);
    assert!(error.contains(&dir.display().to_string()), "{error}");
    assert!(error.contains("Domios does not own"), "{error}");
}

#[tokio::test]
async fn browser_process_rejects_non_loopback_endpoint() {
    let (binary, args) = fake_browser(
        "DevTools listening on ws://10.0.0.5:9222/devtools/browser/x",
        true,
    );
    let error =
        BrowserProcess::spawn_with_stdin(Path::new(binary), &args, READY_DEADLINE, Stdio::piped())
            .await
            .unwrap_err()
            .to_string();
    assert!(error.contains("loopback"), "{error}");
}

#[tokio::test]
async fn browser_process_missing_binary_fails_fast() {
    let error = BrowserProcess::spawn(Path::new("/definitely/not/a/browser"), &[], READY_DEADLINE)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.starts_with("browser spawn failed"), "{error}");
}
