#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
#[path = "support/json_output.rs"]
mod json_output;

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn command(root: &std::path::Path, args: &[&str], key: &str) -> Output {
    let mut child = Command::new(codeterm_bin::codeterm_bin())
        .args(args)
        .env("CODETERM_DATA_DIR", root)
        .env("CODETERM_INSTANCE", "key-fixture")
        .env(
            "CODETERM_SOCKET_PATH",
            format!("absent-{}", uuid::Uuid::new_v4()),
        )
        .env("CODETERM_API_URL", "http://127.0.0.1:1")
        .env_remove("CODETERM_API_TOKEN")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(key.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn mem_embedder_key_set_is_offline_atomic_owner_only_and_never_echoes_the_key() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("data");
    let path = root.join("secrets/openai-embedder.key");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "old-fake-key").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    for (key, json) in [("first-fake-key", false), ("replacement-fake-key", true)] {
        let mut args = vec!["mem", "embedder", "key", "set", "openai"];
        if json {
            args.push("--json");
        }
        let output = command(&root, &args, &format!("{key}\r\n"));
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert!(!stdout.contains(key));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(key));
        let canonical = path.canonicalize().unwrap();
        if json {
            let value = json_output::result(stdout.as_bytes());
            assert_eq!(
                value,
                serde_json::json!({"ok":true,"provider":"openai","path":canonical})
            );
        } else {
            assert_eq!(
                stdout,
                format!("Stored embedder key at {}\n", canonical.display())
            );
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), key);
        assert!(codeterm_util::private_file::owner_only(&path).unwrap());
        assert!(codeterm_util::private_file::owner_only(&root.join("secrets")).unwrap());
        assert_eq!(std::fs::read_dir(root.join("secrets")).unwrap().count(), 1);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
    let output = command(
        &root,
        &[
            "mem", "embedder", "key", "set", "openai", "--mesh", "missing", "--json",
        ],
        "",
    );
    assert_eq!(output.status.code(), Some(7));
    let error: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(error["ok"], false);
    assert!(error["error"]["message"]
        .as_str()
        .unwrap()
        .contains("local-only"));
}
