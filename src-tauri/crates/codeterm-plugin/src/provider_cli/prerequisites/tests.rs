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

#[test]
fn conditional_powershell_installer_with_builtin_fallback_has_no_missing_prerequisites() {
    let native_fallback = "if (Get-Command npm -ErrorAction SilentlyContinue) { npm install -g opaque-pkg } else { $dir = Join-Path $HOME '.opaque\\bin'; New-Item -ItemType Directory -Force -Path $dir | Out-Null; Invoke-WebRequest -Uri 'https://example.test/opaque.zip' -OutFile $zip -UseBasicParsing; Expand-Archive -Path $zip -DestinationPath $dir -Force }";
    assert_eq!(command_binaries(native_fallback), Vec::<String>::new());

    let error_only_else =
        "if (Get-Command npm -ErrorAction SilentlyContinue) { npm install -g opaque-pkg } else { Write-Error 'npm required'; exit 1 }";
    assert_eq!(command_binaries(error_only_else), vec!["npm"]);

    let external_tool_else =
        "if (Get-Command npm -ErrorAction SilentlyContinue) { npm install -g opaque-pkg } else { $ErrorActionPreference = 'Stop'; winget install Opaque.Tool }";
    assert_eq!(command_binaries(external_tool_else), vec!["npm", "winget"]);
}

#[test]
fn conditional_powershell_release_zip_fallback_has_no_missing_prerequisites() {
    let command = "if (Get-Command npm -ErrorAction SilentlyContinue) { npm install -g opaque-cli } else { $ErrorActionPreference = 'Stop'; $repo = 'https://example.test/releases'; $arch = 'x64-baseline'; if ($env:PROCESSOR_ARCHITECTURE -eq 'ARM64') { $arch = 'arm64' }; $asset = \"opaque-windows-$arch.zip\"; $v = $null; try { $pkg = Invoke-RestMethod -Uri 'https://registry.example.test/opaque-cli/latest' -TimeoutSec 15; if ($pkg.version -match '^1\\.\\d+\\.\\d+$') { $null = Invoke-WebRequest -Method Head -Uri \"$repo/download/v$($pkg.version)/$asset\" -UseBasicParsing -TimeoutSec 15; $v = $pkg.version } } catch {}; if (-not $v) { $rels = Invoke-RestMethod -Uri 'https://api.example.test/releases?per_page=100' -Headers @{ 'User-Agent' = 'domios' } -TimeoutSec 30; $rel = $rels | Where-Object { $_.tag_name -match '^v1\\.\\d+\\.\\d+$' } | Select-Object -First 1; if ($rel) { $v = $rel.tag_name.TrimStart('v') } }; if (-not $v) { throw 'Could not determine release' }; $dir = Join-Path $HOME '.opaque\\bin'; New-Item -ItemType Directory -Force -Path $dir | Out-Null; $zip = Join-Path $env:TEMP \"opaque-$v.zip\"; try { Invoke-WebRequest -Uri \"$repo/download/v$v/$asset\" -OutFile $zip -UseBasicParsing; Expand-Archive -Path $zip -DestinationPath $dir -Force } finally { Remove-Item $zip -ErrorAction SilentlyContinue }; $userPath = [Environment]::GetEnvironmentVariable('Path', 'User'); if (-not $userPath -or -not (($userPath -split ';') -contains $dir)) { $nextPath = $dir; if ($userPath) { $nextPath = \"$userPath;$dir\" }; [Environment]::SetEnvironmentVariable('Path', $nextPath, 'User') } }";
    assert_eq!(command_binaries(command), Vec::<String>::new());
}

#[test]
fn setup_hints_include_official_install_routes_for_common_runners() {
    assert!(setup_hint("npm").contains("https://nodejs.org/en/download"));
    assert!(setup_hint("npx").contains("https://nodejs.org/en/download"));
    assert!(setup_hint("node").contains("https://nodejs.org/en/download"));
    assert!(setup_hint("brew").contains("https://brew.sh"));
    assert!(setup_hint("winget")
        .contains("https://learn.microsoft.com/windows/package-manager/winget/"));
}
