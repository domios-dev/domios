use super::*;

#[test]
fn powershell_integration_loads_as_text_with_the_path_quoted() {
    let command =
        powershell_integration_command(std::path::Path::new("C:/Users/o'neil/.domios/ps.ps1"));
    assert_eq!(
        command,
        "Invoke-Expression ([System.IO.File]::ReadAllText('C:/Users/o''neil/.domios/ps.ps1'))"
    );
}

/// The portable-pty invariant behind the prod crash: a default-prog (login)
/// builder panics the instant an arg is added.
#[test]
#[should_panic(expected = "default_prog")]
fn default_prog_builder_panics_on_arg() {
    let mut cmd = CommandBuilder::new_default_prog();
    cmd.arg("--rcfile");
}

/// Regression guard. The plain terminal is built with `new_default_prog()`
/// (a login shell). Before the fix the bash branch called
/// `cmd.arg("--rcfile")` on it, panicking `spawn_pty` and leaving bash
/// terminals blank — zsh used env only, so it was spared (the "x64 vs arm"
/// report was really bash vs zsh). `apply_env` must never add args to a
/// default-prog builder.
#[test]
fn apply_env_never_adds_args_to_default_prog_bash() {
    ensure_scripts(); // make the bashrc integration file exist — the panic precondition
    let mut cmd = CommandBuilder::new_default_prog();
    apply_env(&mut cmd, "/bin/bash"); // must not panic
    assert!(
        cmd.is_default_prog(),
        "apply_env must not add args to the default login-shell builder"
    );
}

/// An explicit-prog builder (specific shell/command) safely takes --rcfile.
#[test]
fn apply_env_keeps_explicit_bash_prog_spawnable() {
    ensure_scripts();
    let mut cmd = CommandBuilder::new("/bin/bash");
    apply_env(&mut cmd, "/bin/bash"); // must not panic
    assert!(!cmd.is_default_prog());
}

#[cfg(unix)]
#[test]
fn bash_host_path_reporting_uses_the_shell_mount_mapping() {
    let home = tempfile::tempdir().unwrap();
    let harness =
        format!("cygpath() {{ printf '%s' 'C:/repo with spaces'; }}\n{BASH_RC}\n__ct_update_cwd");
    let mut command = std::process::Command::new("/bin/bash");
    command
        .args(["--noprofile", "--norc", "-c", &harness])
        .env("HOME", home.path())
        .env("CODETERM_HOST_PATHS", "1")
        .env_remove("CODETERM_SHELL_INTEGRATION");
    codeterm_util::process_util::no_window_std(&mut command);
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("633;P;Cwd=C:/repo with spaces"));
    assert!(stdout.contains("/C:/repo with spaces\x1b\\"));
}

#[cfg(target_os = "windows")]
#[test]
fn apply_env_wires_powershell_command_finished_observation() {
    ensure_scripts();
    let script = integration_dir().join("powershell.ps1");
    // The shipped script, not this machine's copy: ensure_scripts discards its
    // write error, so an install could be running an older file than this constant.
    let content = POWERSHELL_PS1;
    let authenticated_command = content.find("']633;E;;'").unwrap();
    let command_finished = content.find("']633;D;'").unwrap();
    assert!(content.contains("$env:CODETERM_SHELL_NONCE"));
    assert!(content.contains("Remove-Item Env:CODETERM_SHELL_NONCE"));
    assert!(content.contains("IsNullOrEmpty($global:__CodeTermShellNonce)"));
    assert!(authenticated_command < command_finished);

    let mut cmd = CommandBuilder::new("powershell.exe");
    apply_env(&mut cmd, "powershell.exe");
    let argv = cmd.get_argv();
    assert_eq!(argv[1], "-NoExit");
    assert_eq!(argv[2], "-Command");
    assert_eq!(
        argv[3].to_string_lossy(),
        powershell_integration_command(&script)
    );
}

#[cfg(target_os = "windows")]
#[test]
fn only_agent_powershell_skips_profiles_and_keeps_integration() {
    ensure_scripts();
    for shell in ["powershell.exe", "pwsh.exe", "cmd.exe", "bash.exe"] {
        for agent_shell in [false, true] {
            let mut cmd = CommandBuilder::new(shell);
            cmd.env("PATH", "inherited-provider-path");
            cmd.env("OPAQUE_AUTH", "inherited-auth");
            apply_agent_startup(&mut cmd, shell, agent_shell);
            apply_env(&mut cmd, shell);
            assert_eq!(
                cmd.get_argv().iter().any(|arg| arg == "-NoProfile"),
                agent_shell && is_powershell(shell)
            );
            assert_eq!(
                cmd.get_env("PATH"),
                Some(std::ffi::OsStr::new("inherited-provider-path"))
            );
            assert_eq!(
                cmd.get_env("OPAQUE_AUTH"),
                Some(std::ffi::OsStr::new("inherited-auth"))
            );
            if is_powershell(shell) {
                assert_eq!(
                    cmd.get_argv().last().unwrap().to_string_lossy(),
                    powershell_integration_command(&integration_dir().join("powershell.ps1"))
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
#[test]
fn restore_contract_powershell_command_finished_requires_a_new_history_entry() {
    // The shipped script, for the same reason as the test above.
    let content = POWERSHELL_PS1;
    let prompt = content.find("function global:prompt").unwrap();
    let history = content.find("Get-History -Count 1").unwrap();
    let gate = content
        .find("$historyId -gt $global:__CodeTermLastHistoryId")
        .unwrap();
    let authenticated_command = prompt + content[prompt..].find("']633;E;;'").unwrap();
    assert!(prompt < history && history < gate && gate < authenticated_command);
    assert!(!content.contains("__CodeTermPromptSeen"));
}

#[cfg(target_os = "windows")]
#[test]
fn apply_env_keeps_default_powershell_builders_argument_free() {
    ensure_scripts();
    for shell in ["powershell.exe", "pwsh.exe"] {
        let mut cmd = CommandBuilder::new_default_prog();
        apply_env(&mut cmd, shell);
        assert!(cmd.is_default_prog());
        assert!(cmd.get_argv().is_empty());
    }
}
#[cfg(target_os = "windows")]
#[test]
fn shell_integration_powershell_chains_readline_and_prompt() {
    let harness = r#"
$env:CODETERM_SHELL_NONCE = 'own-nonce'
function global:prompt { 'custom-prompt' }
function global:PSConsoleHostReadLine { param($token) 'line-' + $token }
"#;
    let assertions = r#"
$line = PSConsoleHostReadLine 'argument'
if ($line -ne 'line-argument') { throw 'ReadLine result or arguments changed' }
if ((prompt) -ne 'custom-prompt') { throw 'Prompt result changed' }
$global:__CodeTermOriginalReadLine = { '' }
if ((PSConsoleHostReadLine) -ne '') { throw 'Empty input changed' }
"#;
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!("{harness}\n{POWERSHELL_PS1}\n{assertions}"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert_eq!(
        stdout
            .matches("\x1b]633;E;line-argument;own-nonce\x07\x1b]633;C\x07")
            .count(),
        1
    );
    assert!(stdout.contains("\x1b]633;A\x07\x1b]633;B\x07"));
}

#[test]
fn generated_startup_scripts_guard_user_sources_before_loading() {
    let dir = tempfile::tempdir().unwrap();
    write_scripts(dir.path());
    for file in [".zshenv", ".zprofile", ".zlogin", ".zshrc"] {
        let script = std::fs::read_to_string(dir.path().join(file)).unwrap();
        let guard = format!("__ct_loading_{}=1", file.trim_start_matches('.'));
        assert!(script.find(&guard).unwrap() < script.find("source ").unwrap());
        assert!(script.contains("CODETERM_USER_ZDOTDIR=\"$HOME\""));
        assert!(script.contains("-ef \"${(%):-%x}\""));
    }
    let bash = std::fs::read_to_string(dir.path().join("bashrc")).unwrap();
    assert!(
        bash.find("__ct_loading_bashrc=1").unwrap() < bash.find(". \"$HOME/.bashrc\"").unwrap()
    );
    assert!(bash.contains("-ef \"${BASH_SOURCE[0]}\""));
    let fish =
        std::fs::read_to_string(dir.path().join("fish/vendor_conf.d/00-codeterm.fish")).unwrap();
    assert!(fish.contains("set -q CODETERM_SHELL_INTEGRATION; and return"));
    let powershell = std::fs::read_to_string(dir.path().join("powershell.ps1")).unwrap();
    assert!(
        powershell
            .find("if ($global:__CodeTermShellIntegrationLoaded) { return }")
            .unwrap()
            < powershell.find("$env:CODETERM_CLI_PATH").unwrap()
    );
}

#[cfg(target_os = "macos")]
#[test]
fn generated_zsh_scripts_reject_inherited_self_reference_and_keep_custom_config() {
    let root = tempfile::tempdir().unwrap();
    let integration = root.path().join("shell-integration");
    let home = root.path().join("home");
    let custom = root.path().join("custom");
    for dir in [&integration, &home, &custom] {
        std::fs::create_dir_all(dir).unwrap();
    }
    write_scripts(&integration);
    for dir in [&home, &custom] {
        for file in [".zshenv", ".zprofile", ".zlogin", ".zshrc"] {
            std::fs::write(dir.join(file), "(( startup_count += 1 ))\n").unwrap();
        }
    }
    let alias = root.path().join("alias");
    std::os::unix::fs::symlink(&integration, &alias).unwrap();
    for (user, expected) in [(&integration, &home), (&alias, &home), (&custom, &custom)] {
        let output = std::process::Command::new("/bin/zsh")
            .args(["-d", "-f", "-c", "startup_count=0; for file in .zshenv .zprofile .zshrc .zlogin; do source \"$CODETERM_INTEGRATION_ZDOTDIR/$file\"; done; print -r -- \"$startup_count|$CODETERM_USER_ZDOTDIR\""])
            .env_clear().env("PATH", "/usr/bin:/bin")
            .env("HOME", &home).env("ZDOTDIR", &integration)
            .env("CODETERM_INTEGRATION_ZDOTDIR", &integration)
            .env("CODETERM_USER_ZDOTDIR", user).env("CODETERM_ORIG_ZDOTDIR", user)
            .output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(&format!("4|{}", expected.display()))
        );
        assert!(
            output.stderr.is_empty(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
