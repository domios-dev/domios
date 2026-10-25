use super::*;
use crate::probe::{cancellation::Cancellation, owner::ProbeResourceOwner, test_receipt::Receipt};
use codeterm_blueprint_types::blueprint_execution::ExecutionEnvelope;
use std::sync::Arc;

fn inherited_with_path(path: OsString) -> Vec<(OsString, OsString)> {
    let mut inherited: Vec<_> = std::env::vars_os()
        .filter(|(key, _)| !key.eq_ignore_ascii_case("PATH"))
        .collect();
    inherited.push(("PATH".into(), path));
    inherited
}

#[test]
fn blueprint_v2_runtime_command_path_preserves_tools_and_excludes_cli() {
    let root = tempfile::tempdir().unwrap();
    let directories = [
        "tools",
        "instance",
        "codeterm-cli",
        "domios-cli",
        "defaults",
    ]
    .map(|name| root.path().join(name));
    let [tools, instance, codeterm, domios, defaults] = &directories;
    let path = std::env::join_paths([tools, instance, codeterm, domios, tools]).unwrap();
    let mut inherited = inherited_with_path(path);
    inherited.push(("CODETERM_CLI_PATH".into(), codeterm.join("codeterm").into()));
    inherited.push(("DoMiOs_Cli_Path".into(), domios.join("domios").into()));
    let environment = Environment::for_command(
        inherited,
        Some(&instance.join("domios")),
        &[defaults.to_str().unwrap(), tools.to_str().unwrap()],
    );
    let (_, path) = environment
        .variables
        .iter()
        .find(|(key, _)| key == "PATH")
        .unwrap();
    let path: Vec<_> = std::env::split_paths(path).collect();
    assert_eq!(
        path.first(),
        Some(tools),
        "inherited user tools keep precedence"
    );
    assert_eq!(path.iter().filter(|path| *path == tools).count(), 2);
    assert!(path.contains(defaults));
    for cli in [instance, codeterm, domios] {
        assert!(
            !path.contains(cli),
            "CLI directory leaked: {}",
            cli.display()
        );
    }
}

#[test]
fn blueprint_v2_runtime_shared_directory_preserves_runtimes_and_shadows_cli() {
    let root = tempfile::tempdir().unwrap();
    let shared = root.path().join("usr-bin");
    std::fs::create_dir(&shared).unwrap();
    for name in ["python3", "node", "sh", "domios", "codeterm"] {
        std::fs::write(shared.join(name), "opaque-tool").unwrap();
    }
    let environment = Environment::for_command_on_platform(
        [("PATH".into(), shared.as_os_str().to_owned())],
        None,
        &[],
        false,
    );
    let path = &environment
        .variables
        .iter()
        .find(|(key, _)| key == "PATH")
        .unwrap()
        .1;
    let directories: Vec<_> = std::env::split_paths(path).collect();
    assert_eq!(directories[1], shared);
    for name in ["python3", "node", "sh"] {
        assert_eq!(
            environment.resolve_program(&[name]),
            Some(shared.join(name).canonicalize().unwrap())
        );
    }
    for name in ["domios", "codeterm"] {
        let stub = environment.resolve_program(&[name]).unwrap();
        assert_eq!(stub, directories[0].join(name).canonicalize().unwrap());
        #[cfg(unix)]
        assert_eq!(
            std::process::Command::new(stub).status().unwrap().code(),
            Some(126)
        );
    }
    let clone = environment.clone();
    drop(environment);
    assert!(clone.resolve_program(&["domios"]).is_some());
}

#[cfg(unix)]
#[test]
fn blueprint_v2_runtime_package_cli_keeps_usr_bin() {
    let environment = Environment::for_command_on_platform(
        [("PATH".into(), "/usr/bin:/bin".into())],
        Some(Path::new("/usr/bin/domios")),
        &[],
        false,
    );
    let path = &environment
        .variables
        .iter()
        .find(|(key, _)| key == "PATH")
        .unwrap()
        .1;
    assert!(std::env::split_paths(path).any(|path| path == Path::new("/usr/bin")));
    assert!(environment.resolve_program(&["sh"]).is_some());
}

#[test]
fn blueprint_v2_runtime_command_child_resolves_system_and_user_tools() {
    let root = tempfile::tempdir().unwrap();
    let tools = root.path().join("user-tools");
    let cli = root.path().join("instance-cli");
    std::fs::create_dir(&tools).unwrap();
    std::fs::create_dir(&cli).unwrap();
    std::fs::write(root.path().join("input.txt"), "cat-resolved\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let tool = tools.join("blu41-user-tool");
        std::fs::write(&tool, "#!/bin/sh\nprintf 'user-tool-resolved\\n'\n").unwrap();
        std::fs::set_permissions(tool, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[cfg(windows)]
    std::fs::write(
        tools.join("blu41-user-tool.cmd"),
        "@echo off\r\necho user-tool-resolved\r\n",
    )
    .unwrap();
    let inherited_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        [tools.clone(), cli.clone()]
            .into_iter()
            .chain(std::env::split_paths(&inherited_path)),
    )
    .unwrap();
    let mut inherited = inherited_with_path(path);
    inherited.push(("CODETERM_API_TOKEN".into(), "opaque-secret".into()));
    inherited.push(("domios_future_selector".into(), "opaque-selector".into()));
    let environment = Environment::for_command(inherited, Some(&cli.join("domios")), &[]);
    let (_, child_path) = environment
        .variables
        .iter()
        .find(|(key, _)| key == "PATH")
        .unwrap();
    assert!(!std::env::split_paths(child_path).any(|path| path == cli));
    assert!(environment.variables.iter().all(|(key, _)| {
        let key = key.to_string_lossy().to_ascii_uppercase();
        key != "CODETERM"
            && key != "DOMIOS"
            && !key.starts_with("CODETERM_")
            && !key.starts_with("DOMIOS_")
    }));
    #[cfg(unix)]
    let command = "cat input.txt && git --version && sh -c \"printf 'sh-resolved\\n'\" && blu41-user-tool; env";
    #[cfg(windows)]
    let command = "$ErrorActionPreference='Stop'; cat input.txt; git --version; if ($LASTEXITCODE) { exit $LASTEXITCODE }; blu41-user-tool; if ($LASTEXITCODE) { exit $LASTEXITCODE }; Get-ChildItem Env: | ForEach-Object { '{0}={1}' -f $_.Name,$_.Value }";
    let owner = Arc::new(ProbeResourceOwner::new());
    let cancellation = Cancellation::default();
    let receipt = Receipt::start("command-path", &cancellation);
    let result = crate::probe::run_enveloped(
        command,
        root.path(),
        crate::probe::PROBE_RUNTIME,
        crate::probe::PROBE_RUNTIME,
        &owner,
        &environment,
        &ExecutionEnvelope::from_text("opaque-input"),
        &cancellation,
    );
    let details = match &result {
        Ok(output) => format!(
            "exit={:?} stderr_tail={}",
            output.exit_code,
            crate::probe::test_receipt::stderr_tail(&output.stderr_text())
        ),
        Err(error) => format!("{error:?}"),
    };
    let receipt = receipt.finish(
        result.as_ref().is_ok_and(|output| output.success),
        &details,
        &details,
    );
    let output = result.unwrap_or_else(|failure| panic!("{failure:?}; {}", receipt.diagnostic));
    assert!(output.success, "{}", receipt.diagnostic);
    let stdout = output.stdout_text();
    for expected in ["cat-resolved", "git version", "user-tool-resolved"] {
        assert!(
            stdout.contains(expected),
            "missing {expected}: {stdout}; {}",
            receipt.diagnostic
        );
    }
    #[cfg(unix)]
    assert!(stdout.contains("sh-resolved"), "{stdout}");
    for line in stdout.lines() {
        let key = line.split('=').next().unwrap().to_ascii_uppercase();
        assert!(
            key != "CODETERM"
                && key != "DOMIOS"
                && !key.starts_with("CODETERM_")
                && !key.starts_with("DOMIOS_"),
            "{line}"
        );
    }
    assert!(!stdout.contains(cli.to_string_lossy().as_ref()));
    assert!(!stdout.contains("opaque-secret"));
    assert!(!stdout.contains("opaque-selector"));
    assert_eq!(receipt.child_pids.len(), 1);
    assert_eq!(owner.snapshot().unconfirmed, 0);
}
