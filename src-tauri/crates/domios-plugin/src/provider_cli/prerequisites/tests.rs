use super::*;

#[test]
fn manifest_commands_supply_runner_and_alternative_checks() {
    assert_eq!(
        command_binaries("uv tool install opaque-package"),
        vec!["uv"]
    );
    assert_eq!(command_binaries("if command -v uv; then uv tool install x; elif command -v pipx; then pipx install x; fi"), vec!["pipx", "uv"]);
    assert_eq!(
        command_binaries(
            "if command -v uv >/dev/null 2>&1; then uv tool install x; else pipx install x; fi"
        ),
        vec!["pipx", "uv"]
    );
    assert_eq!(
        command_binaries("uv tool install x || pipx install x"),
        vec!["pipx", "uv"]
    );
    assert_eq!(
        command_binaries(
            "if (Get-Command 'uv' -ErrorAction SilentlyContinue) { uv tool install x }"
        ),
        vec!["uv"]
    );
    assert_eq!(
        command_binaries("if unknown-script; then something; fi"),
        Vec::<String>::new()
    );
}

#[test]
fn powershell_builtins_are_not_prerequisites() {
    assert_eq!(
        command_binaries("irm https://x.ai/cli/install.ps1 | iex"),
        Vec::<String>::new()
    );
    assert_eq!(
            command_binaries("$s = Join-Path $env:TEMP 'a.ps1'; Invoke-WebRequest -Uri 'https://e/x' -OutFile $s; & $s"),
            Vec::<String>::new()
        );
}

#[test]
fn missing_prerequisites_are_named_with_setup_guidance_without_guessing_unknown() {
    let mut prerequisites = vec![
        CommandPrerequisite {
            binary: "uv".into(),
            installed: Some(false),
            hint: setup_hint("uv"),
        },
        CommandPrerequisite {
            binary: "pipx".into(),
            installed: Some(false),
            hint: setup_hint("pipx"),
        },
    ];
    let hint = missing_prerequisite_hint(&prerequisites).unwrap();
    assert!(hint.contains("uv, pipx"));
    assert!(hint.contains("https://docs.astral.sh/uv/"));
    assert!(hint.contains("https://pipx.pypa.io/"));
    prerequisites[0].installed = Some(true);
    prerequisites[1].installed = None;
    assert_eq!(missing_prerequisite_hint(&prerequisites), None);
}

#[test]
fn updater_arguments_are_not_command_prerequisites() {
    for command in [
        "grok update --version {version}",
        "fixture-cli upgrade {version}",
        "grok update --version latest",
        "grok update --message which imaginary",
        "grok update --message command -v imaginary",
        "grok update --version={version} --channel stable",
        "grok update --message 'release; candidate | preview & beta {version}'",
        "grok update --version {version}; other update --version latest",
    ] {
        let expected = if command.contains("; other") {
            vec!["grok", "other"]
        } else if command.starts_with("fixture-cli") {
            vec!["fixture-cli"]
        } else {
            vec!["grok"]
        };
        assert_eq!(command_binaries(command), expected, "{command}");
    }
}
