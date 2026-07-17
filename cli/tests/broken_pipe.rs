use std::process::{Command, Stdio};

#[test]
fn exits_successfully_when_an_early_stdout_consumer_closes() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_codeterm"))
        .args(["doc", "API"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codeterm");

    drop(child.stdout.take());

    let output = child.wait_with_output().expect("wait for codeterm");
    assert!(
        output.status.success(),
        "closed stdout must be a successful no-op; stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
