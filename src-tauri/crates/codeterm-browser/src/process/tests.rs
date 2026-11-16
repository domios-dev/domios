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

#[tokio::test]
async fn browser_process_exit_before_devtools_is_an_error() {
    let (binary, args) = fake_browser("Opening in existing browser session.", false);
    let error = BrowserProcess::spawn(Path::new(binary), &args, READY_DEADLINE)
        .await
        .unwrap_err()
        .to_string();
    assert!(error.contains("before DevTools was ready"), "{error}");
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
