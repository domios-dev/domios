#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

use std::process::Stdio;

#[test]
fn secret_delete_non_tty_fails_fast() {
    let output = codeterm_bin::command()
        .args(["mem", "secret", "delete", "--name", "no-such-secret"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .expect("run codeterm");
    assert!(
        !output.status.success(),
        "delete without --yes on non-tty stdin must fail"
    );
}
