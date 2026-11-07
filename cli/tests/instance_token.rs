#[path = "support/codeterm_bin.rs"]
mod codeterm_bin;
#[path = "support/json_output.rs"]
mod json_output;

fn command(root: &std::path::Path) -> std::process::Command {
    let mut command = codeterm_bin::command();
    command
        .env("CODETERM_DATA_DIR", root)
        .env(
            "CODETERM_INSTANCE",
            format!("token-test-{}", std::process::id()),
        )
        .env(
            "CODETERM_API_PORT",
            std::net::TcpListener::bind("127.0.0.1:0")
                .unwrap()
                .local_addr()
                .unwrap()
                .port()
                .to_string(),
        )
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
        let json = flags.contains(&"--json");
        let output = command(dir.path())
            .env_remove("CODETERM_API_TOKEN")
            .args(flags)
            .output()
            .unwrap();
        assert!(output.status.success());
        if json {
            assert_eq!(json_output::result(&output.stdout), "isolated-authority");
        } else {
            assert_eq!(output.stdout, b"isolated-authority");
        }
        assert!(output.stderr.is_empty());
    }
    let inherited = command(dir.path())
        .args(["system", "api-token"])
        .output()
        .unwrap();
    assert!(!inherited.status.success());
    assert!(inherited.stdout.is_empty());
    let error = String::from_utf8_lossy(&inherited.stderr);
    assert!(!error.contains("isolated-authority"));
    assert!(!error.contains("foreign-inherited"));
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
    for marker in codeterm_util::tab_launch::MARKERS {
        let output = command(dir.path())
            .env_remove("CODETERM_API_TOKEN")
            .env(marker, "isolated-tab")
            .args(["system", "api-token"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{marker}");
        assert!(output.stdout.is_empty(), "{marker}");
        assert!(
            !String::from_utf8_lossy(&output.stderr).contains("isolated-host-authority"),
            "{marker}"
        );
    }
}
