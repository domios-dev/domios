use super::*;

#[cfg(unix)]
#[test]
fn login_and_tool_shells_resolve_the_instance_cli_after_profiles() {
    use std::os::unix::fs::PermissionsExt;

    let root = tempfile::tempdir().unwrap();
    let integration = root.path().join("integration");
    let home = root.path().join("home");
    let own = root.path().join("own bin");
    let foreign = root.path().join("foreign bin");
    for dir in [&integration, &home, &own, &foreign] {
        std::fs::create_dir_all(dir).unwrap();
    }
    for (dir, marker) in [(&own, "owner"), (&foreign, "foreign")] {
        let binary = dir.join("codeterm");
        std::fs::write(&binary, format!("#!/bin/sh\nprintf '{marker}'\n")).unwrap();
        std::fs::set_permissions(binary, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for profile in [".zprofile", ".zshrc", ".zlogin", ".bashrc"] {
        std::fs::write(
            home.join(profile),
            format!("export PATH='{}:/usr/bin:/bin'\n", foreign.display()),
        )
        .unwrap();
    }
    write_scripts(&integration);
    for (shell, args) in [
        ("/bin/bash", vec!["-c", "codeterm"]),
        ("/bin/bash", vec!["-lc", "codeterm"]),
        ("/bin/zsh", vec!["-lc", "codeterm"]),
        ("/bin/zsh", vec!["-lic", "/bin/zsh -lc codeterm"]),
    ] {
        if !std::path::Path::new(shell).is_file() {
            continue;
        }
        let output = std::process::Command::new(shell)
            .args(args)
            .env("HOME", &home)
            .env("ZDOTDIR", &integration)
            .env("CODETERM_USER_ZDOTDIR", &home)
            .env("CODETERM_INTEGRATION_ZDOTDIR", &integration)
            .env("BASH_ENV", integration.join("bashrc"))
            .env("CODETERM_CLI_PATH", own.join("codeterm"))
            .env("PATH", &foreign)
            .output()
            .unwrap();
        assert!(output.status.success(), "{shell}: {:?}", output);
        assert_eq!(String::from_utf8_lossy(&output.stdout), "owner", "{shell}");
    }
}

#[cfg(windows)]
#[test]
fn powershell_resolves_the_instance_cli_after_profile_path_replacement() {
    let root = tempfile::tempdir().unwrap();
    let own = root.path().join("own bin");
    let foreign = root.path().join("foreign bin");
    for dir in [&own, &foreign] {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("codeterm.cmd"), "@echo off\r\nexit /b 0\r\n").unwrap();
    }
    let script = root.path().join("integration.ps1");
    std::fs::write(&script, POWERSHELL_PS1).unwrap();
    let output = std::process::Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Command"])
        .arg(format!(
            "$env:PATH='{}'; {}; (Get-Command codeterm).Source",
            foreign.display(),
            powershell_integration_command(&script)
        ))
        .env("CODETERM_CLI_PATH", own.join("codeterm.exe"))
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim(),
        own.join("codeterm.cmd").to_str().unwrap()
    );
}
