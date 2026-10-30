#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;

fn command(root: &std::path::Path) -> std::process::Command {
    let mut command = std::process::Command::new(codeterm_bin::codeterm_bin());
    for (key, _) in std::env::vars() {
        if key.starts_with("CODETERM_") {
            command.env_remove(key);
        }
    }
    command
        .env("CODETERM_DATA_DIR", root)
        .env("CODETERM_API_TOKEN", "foreign-inherited");
    command
}

#[test]
fn instance_token_command_prints_only_local_authority_without_a_daemon() {
    let dir = tempfile::tempdir().unwrap();
    codeterm_util::secret_file::SecretFile::new_at(dir.path(), "domios.instance")
        .set("api_token", "isolated-authority")
        .unwrap();
    for flags in [
        vec!["system", "api-token"],
        vec!["--json", "system", "api-token"],
    ] {
        let output = command(dir.path()).args(flags).output().unwrap();
        assert!(output.status.success());
        assert_eq!(output.stdout, b"isolated-authority");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn instance_token_command_refuses_mesh_without_opening_the_local_store() {
    let dir = tempfile::tempdir().unwrap();
    let absent = dir.path().join("must-not-be-created");
    let output = command(&absent)
        .args(["--mesh", "foreign", "system", "api-token"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(7));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("foreign-inherited"));
    assert!(!absent.exists());
}

#[test]
fn instance_token_command_refuses_tab_context_without_disclosing_host_authority() {
    let dir = tempfile::tempdir().unwrap();
    codeterm_util::secret_file::SecretFile::new_at(dir.path(), "domios.instance")
        .set("api_token", "isolated-host-authority")
        .unwrap();
    let output = command(dir.path())
        .env("CODETERM_TAB_ID", "isolated-tab")
        .args(["system", "api-token"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("isolated-host-authority"));
}
