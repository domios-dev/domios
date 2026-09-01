//! Mechanism oracles for `mem secret save-master-password`: writes no credential
//! file, asks the backend before claiming, and never emits the credential.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// Obviously not a credential; supplied as credential data on stdin.
const SYNTHETIC_SECRET: &str = "synthetic-not-a-credential-0000";

fn cli_src_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

/// The `save-master-password` arm alone, sliced out of the command dispatcher.
fn save_master_password_arm() -> String {
    let path = cli_src_root().join("cmd").join("mem_secret.rs");
    let body = std::fs::read_to_string(&path).expect("read mem_secret.rs");
    let start = body
        .find("MemSecretAction::SaveMasterPassword")
        .expect("mem_secret.rs handles SaveMasterPassword");
    let end = body[start..]
        .find("fn read_stdin")
        .map(|i| start + i)
        .unwrap_or(body.len());
    body[start..end].to_string()
}

#[test]
fn save_master_password_writes_no_credential_file() {
    let arm = save_master_password_arm();
    assert!(
        !arm.contains("std::fs::"),
        "the arm must not touch the filesystem; a credential it writes has no reader"
    );
    assert!(
        !arm.contains("bitwarden-master.env"),
        "the retired credential drop must not be named on this path"
    );
}

#[test]
fn save_master_password_consults_the_backend_before_claiming_anything() {
    // The defect was claiming auto-unlock without ever asking the backend.
    let arm = save_master_password_arm();
    assert!(
        arm.contains("/api/secrets/status"),
        "the arm must read the backend's declaration before it reports an outcome"
    );
}

/// A home the CLI can write into, so a regression that writes a credential file
/// lands here instead of the machine's real home.
fn sandbox_home(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("codeterm-cli-home-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("sandbox home");
    dir
}

fn contains_secret(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let hit = if path.is_dir() {
            contains_secret(&path)
        } else {
            String::from_utf8_lossy(&std::fs::read(&path).unwrap_or_default())
                .contains(SYNTHETIC_SECRET)
        };
        if hit {
            return true;
        }
    }
    false
}

fn run_save_master_password(home: &Path, json: bool) -> (String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_codeterm"));
    // Point at a port nothing serves so the command cannot reach a real vault.
    cmd.env_remove("CODETERM_API_TOKEN")
        .env_remove("CODETERM_SOCKET_PATH")
        .env("CODETERM_API_URL", "http://127.0.0.1:1")
        .env("CODETERM_API_PORT", "1")
        .env("HOME", home)
        .env("USERPROFILE", home);
    if json {
        cmd.arg("--json");
    }
    cmd.args(["mem", "secret", "save-master-password"]);
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn codeterm");
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().expect("stdin");
        let _ = stdin.write_all(SYNTHETIC_SECRET.as_bytes());
    }
    let out = child.wait_with_output().expect("wait for codeterm");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
    )
}

#[test]
fn credential_never_reaches_an_output_stream() {
    for (tag, json) in [("plain", false), ("json", true)] {
        let home = sandbox_home(tag);
        let (stdout, stderr) = run_save_master_password(&home, json);
        assert!(
            !stdout.contains(SYNTHETIC_SECRET),
            "stdout leaked the credential: {stdout}"
        );
        assert!(
            !stderr.contains(SYNTHETIC_SECRET),
            "stderr leaked the credential: {stderr}"
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}

#[test]
fn save_master_password_persists_nothing_locally() {
    let home = sandbox_home("nofile");
    let _ = run_save_master_password(&home, false);
    assert!(
        !contains_secret(&home),
        "the command must not leave the credential anywhere under the home directory"
    );
    let _ = std::fs::remove_dir_all(&home);
}
