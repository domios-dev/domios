use super::*;
use crate::multiplexer::state::MultiplexerState;
use crate::service::test_fixture::ServiceFixture;

#[test]
fn restored_shell_gets_current_credential_after_server_restart_and_nonce_rotation() {
    let (mut mux, tab_id) = MultiplexerState::new();
    let tab = mux.get_tab_mut(&tab_id).unwrap();
    tab.session_binding_nonce = Some("saved-launch".into());
    tab.reported_session_binding_nonce = Some("saved-launch".into());
    tab.agent_type = Some("opaque-provider".into());
    tab.agent_session_id = Some("saved-session".into());
    tab.boot_stage = Some(codeterm_types::pty::BootStage::AgentReady);
    let previous = ServiceFixture::new(mux);
    let inherited = previous.service.frontend_pty_env(Some(&tab_id));
    let old_token = inherited["CODETERM_API_TOKEN"].clone();
    let snapshot = previous.service.mux.read().to_session_snapshot();
    drop(previous);
    let mux = MultiplexerState::from_session_snapshot(&snapshot);
    assert!(mux.get_tab(&tab_id).unwrap().restored_from_snapshot);
    assert_eq!(mux.get_tab(&tab_id).unwrap().agent_session_id.as_deref(), Some("saved-session"));
    let restarted = ServiceFixture::new(mux);
    assert!(restarted.service.identify_tab_credential(&old_token).is_none());
    let shell_env = restarted.service.frontend_pty_env(Some(&tab_id));
    let nonce = restarted.service.reserve_agent_launch_binding_nonce(&tab_id, Some("saved-session"));
    assert!(restarted.service.identify_tab_credential(&shell_env["CODETERM_API_TOKEN"]).is_none());
    let wrapped = wrap_command(&restarted.service, &tab_id, "opaque-provider", &nonce, "attempt", false).unwrap();
    let path = std::fs::read_dir(restarted.service.ensure_agent_pad(&tab_id).unwrap().path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.file_name().unwrap().to_string_lossy().starts_with(".launch-credential-"))
        .unwrap();
    let current = std::fs::read_to_string(&path).unwrap();
    assert!(!wrapped.command.contains(&current));
    assert!(!wrapped.command.contains(&old_token));
    assert!(wrapped.command.contains(&nonce));
    assert!(codeterm_util::private_file::owner_only(&path).unwrap());
    let caller = restarted.service.identify_tab_credential(&current).unwrap();
    assert_eq!(caller.tab_id, tab_id);
    assert_eq!(caller.binding_nonce, nonce);
    for windows in [false, true] {
        let command = with_credential_file("opaque-provider", &path, windows);
        assert!(!command.contains(&current));
        assert!(command.contains("CODETERM_API_TOKEN"));
        assert!(command.contains("opaque-provider"));
    }
}

fn fixture_command() -> (ServiceFixture, String, String, CredentialCommand) {
    let (mux, tab) = MultiplexerState::new();
    let fixture = ServiceFixture::new(mux);
    let nonce = fixture.service.ensure_session_binding_nonce(&tab);
    let command = wrap_command(&fixture.service, &tab, "opaque-provider", &nonce, "attempt", false).unwrap();
    (fixture, tab, nonce, command)
}

#[test]
fn no_write_abort_and_replacement_remove_only_the_owned_file() {
    let (fixture, tab, nonce, mut command) = fixture_command();
    let path = command.file.as_ref().unwrap().path().to_path_buf();
    assert!(codeterm_util::private_file::owner_only(&path).unwrap());
    assert!(command.send(&fixture.service, "absent-pty").is_err());
    drop(command);
    assert!(!path.exists());
    let mut command = wrap_command(&fixture.service, &tab, "opaque-provider", &nonce, "attempt", false).unwrap();
    let path = command.file.as_ref().unwrap().path().to_path_buf();
    command.transfer(&fixture.service, "old-pty").unwrap();
    drop(command);
    assert!(path.exists(), "possible writes retain the file while the launch can consume it");
    fixture.service.finish_launch_credential_file(&tab, "other-pty", &nonce);
    assert!(path.exists());
    let nonce = fixture.service.reserve_agent_launch_binding_nonce(&tab, None);
    assert!(!path.exists(), "replacement revokes the old handoff");
    let replacement = wrap_command(&fixture.service, &tab, "opaque-provider", &nonce, "replacement", false).unwrap();
    let replacement_path = replacement.file.as_ref().unwrap().path().to_path_buf();
    assert!(replacement_path.exists());
    assert!(!path.exists());
    drop(replacement);
    assert!(!replacement_path.exists());
}

#[test]
fn uncertain_handoff_is_retired_by_exact_binding_or_pty_death() {
    for retirement in ["binding", "pty-death"] {
        let (fixture, tab, nonce, mut command) = fixture_command();
        let path = command.file.as_ref().unwrap().path().to_path_buf();
        command.transfer(&fixture.service, "own-pty").unwrap();
        drop(command);
        assert!(path.exists());
        match retirement {
            "binding" => fixture.service.finish_launch_credential_file(&tab, "own-pty", &nonce),
            "pty-death" => fixture.service.void_launch_session_proof(&tab),
            _ => unreachable!(),
        }
        assert!(!path.exists());
    }
}

#[test]
fn service_shutdown_removes_uncertain_file_before_its_independent_directory_is_deleted() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("credential.txt");
    let (fixture, tab, nonce, _command) = fixture_command();
    codeterm_util::private_file::write_atomic(&path, b"opaque-token").unwrap();
    fixture.service.retain_launch_credential_file(&tab, &nonce, "own-pty", CredentialFile(path.clone())).unwrap();
    assert!(path.exists());
    drop(fixture);
    assert!(root.path().exists());
    assert!(!path.exists());
}

fn execute_handoff(command: &str, expected: &str, probe: &Path) -> std::process::Output {
    #[cfg(unix)]
    let mut shell = {
        let mut shell = std::process::Command::new("sh");
        shell.args(["-x", "-c", command]);
        shell
    };
    #[cfg(windows)]
    let mut shell = {
        let mut shell = std::process::Command::new(codeterm_util::windows_powershell::preferred_shell().expect("PowerShell installed"));
        shell.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &format!("Set-PSDebug -Trace 2; {command}")]);
        shell
    };
    shell.env_remove("CODETERM_API_TOKEN").env("EXPECTED_TOKEN", expected)
        .env("PROBE_OUTPUT", probe).output().unwrap()
}

#[test]
fn native_handoff_consumes_private_file_without_tracing_token_and_missing_file_refuses_provider() {
    let (fixture, tab, nonce, _command) = fixture_command();
    let file = credential_file(&fixture.service, &tab, &nonce).unwrap();
    let path = file.path().to_path_buf();
    let token = std::fs::read_to_string(&path).unwrap();
    let probe = fixture.temp.path().join("provider-output.txt");
    #[cfg(unix)]
    let provider = "sh -c 'test \"$CODETERM_API_TOKEN\" = \"$EXPECTED_TOKEN\" && printf provider-ready > \"$PROBE_OUTPUT\"'";
    #[cfg(windows)]
    let provider = "if ($env:CODETERM_API_TOKEN -ne $env:EXPECTED_TOKEN) { throw 'credential mismatch' }; [System.IO.File]::WriteAllText($env:PROBE_OUTPUT, 'provider-ready')";
    let command = with_credential_file(provider, &path, cfg!(windows));
    let output = execute_handoff(&command, &token, &probe);
    assert!(output.status.success());
    assert_eq!(std::fs::read_to_string(&probe).unwrap(), "provider-ready");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&token));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&token));
    assert!(!path.exists());
    std::fs::remove_file(&probe).unwrap();
    let output = execute_handoff(&command, &token, &probe);
    assert!(!output.status.success());
    assert!(!probe.exists());
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&token));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&token));
    codeterm_util::private_file::write_atomic(&path, b"").unwrap();
    let output = execute_handoff(&command, &token, &probe);
    assert!(!output.status.success());
    assert!(!probe.exists());
}
