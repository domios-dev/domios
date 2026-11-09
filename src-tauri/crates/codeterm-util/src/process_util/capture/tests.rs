use super::*;

#[test]
fn capture_retains_exit_and_stderr_instead_of_empty_success() {
    let error = capture_result(
        Some(1),
        false,
        b"",
        b"group container: Operation not permitted",
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "exit Some(1): group container: Operation not permitted"
    );
    assert!(capture_result(None, false, b"partial", b"terminated").is_err());
    assert_eq!(capture_result(Some(0), true, b"{}", b"").unwrap(), "{}");
}

#[test]
fn capture_bounds_output() {
    assert!(capture_result(Some(0), true, &vec![0; 4 * 1024 * 1024 + 1], b"").is_err());
    assert!(capture_result(Some(0), true, b"", &vec![0; 8193]).is_err());
}

fn child_command(mode: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command.args([
        "--exact",
        "process_util::capture::tests::capture_child",
        "--nocapture",
    ]);
    command.env("DOMIOS_CAPTURE_FIXTURE", mode);
    command
}

#[test]
fn capture_child() {
    use std::io::Write;
    match std::env::var("DOMIOS_CAPTURE_FIXTURE").as_deref() {
        Ok("pipes") => {
            std::io::stdout()
                .write_all(&vec![b'x'; 128 * 1024])
                .unwrap();
            std::io::stderr()
                .write_all(b"capture-stderr-marker")
                .unwrap();
        }
        Ok("hold") => {
            let (_release, wait) = std::sync::mpsc::channel::<()>();
            let _ = wait.recv();
        }
        _ => {}
    }
}

#[test]
fn capture_drains_stdout_and_stderr_before_waiting_for_exit() {
    let output = bounded_output(child_command("pipes"), Duration::from_secs(2)).unwrap();
    assert!(output.status.success());
    assert!(output
        .stdout
        .windows(128 * 1024)
        .any(|chunk| chunk.iter().all(|byte| *byte == b'x')));
    assert_eq!(output.stderr, b"capture-stderr-marker");
}

#[test]
fn capture_kills_a_child_that_never_exits_at_the_deadline() {
    let started = std::time::Instant::now();
    let error = bounded_output(child_command("hold"), Duration::from_millis(200)).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn capture_stops_reading_at_the_output_limit() {
    let input = [b'x'; 1024];
    let mut reader = input.as_slice();
    let mut output = Vec::new();
    let error = read_limited(&mut reader, &mut output, 16)
        .await
        .unwrap_err();
    assert_eq!(error.to_string(), "output limit exceeded");
    assert_eq!(output.len(), 17);
    assert_eq!(reader.len(), 1007);
}
