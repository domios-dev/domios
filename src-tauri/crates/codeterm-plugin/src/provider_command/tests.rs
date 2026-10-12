use super::*;

#[test]
fn provider_command_env_shebang_is_structural_and_preserves_unsupported_forms() {
    assert_eq!(
        env_interpreter(b"#!/usr/bin/env node\nopaque"),
        Some("node")
    );
    assert_eq!(
        env_interpreter(b"#! /usr/bin/env python3\r\n"),
        Some("python3")
    );
    for head in [
        b"native".as_slice(),
        b"#!/bin/sh\n",
        b"#!/usr/bin/env -S node --flag\n",
        b"#!/usr/bin/env A=B node\n",
    ] {
        assert_eq!(env_interpreter(head), None);
    }
}

#[test]
fn provider_command_interpreter_keeps_wrapper_path_and_argument_boundaries() {
    let mut command =
        interpreted_command(Path::new("/npm/codex wrapper.js"), Path::new("/nvm/node"));
    command.args(["--version", "opaque $argument"]);
    assert_eq!(command.get_program(), "/nvm/node");
    let args: Vec<_> = command.get_args().collect();
    assert_eq!(
        args,
        ["/npm/codex wrapper.js", "--version", "opaque $argument"]
    );
}

#[test]
fn provider_command_nvm_rescue_orders_only_installed_version_directories() {
    let root = tempfile::tempdir().unwrap();
    for name in ["v20.9.0", "v22.1.0", "v20.10.0", "alias"] {
        std::fs::create_dir(root.path().join(name)).unwrap();
    }
    std::fs::write(root.path().join("v99.0.0"), "not a directory").unwrap();
    assert_eq!(
        versioned_bin_dirs(root.path()),
        ["v22.1.0", "v20.10.0", "v20.9.0"].map(|name| root.path().join(name).join("bin"))
    );
}

#[cfg(unix)]
#[test]
fn provider_command_short_path_wrapper_executes_resolved_interpreter() {
    let root = tempfile::tempdir().unwrap();
    let wrapper = root.path().join("fixture-cli");
    std::fs::write(
        &wrapper,
        "#!/usr/bin/env sh\nprintf 'codex-cli 0.162.1\\n'\n",
    )
    .unwrap();
    let mut command = interpreted_command(&wrapper, Path::new("/bin/sh"));
    let output = command
        .env("PATH", "/nonexistent")
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "codex-cli 0.162.1\n"
    );
}
