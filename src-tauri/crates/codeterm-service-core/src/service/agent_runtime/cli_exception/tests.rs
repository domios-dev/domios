use super::*;
use codeterm_plugin::agent_runtime::RuntimeSettings;

pub fn fixture() -> (
    tempfile::TempDir,
    RuntimeContext,
    ExecutionSandboxPolicy,
    PathBuf,
) {
    let directory = tempfile::tempdir().unwrap();
    let cwd = directory.path().join("project");
    fs::create_dir(&cwd).unwrap();
    let executable = directory.path().join("trusted-cli.exe");
    fs::write(&executable, b"opaque-cli-bytes").unwrap();
    let executable = executable.canonicalize().unwrap();
    let cwd = plain_path(&cwd.canonicalize().unwrap()).unwrap();
    let context = RuntimeContext {
        epoch: 7,
        tab_id: "own-tab".into(),
        binding_nonce: "own-binding".into(),
        cwd: cwd.clone(),
        settings: RuntimeSettings {
            mode: Some("workspace-write".into()),
            network_access: Some(false),
            ..Default::default()
        },
        ..Default::default()
    };
    let sandbox = ExecutionSandboxPolicy {
        intent: ExecutionIntent::Mutating,
        cwd: cwd.clone(),
        launch_args: Vec::new(),
        runtime_mode: Some("workspace-write".into()),
        writable_roots: vec![cwd],
        environment: Default::default(),
    };
    (directory, context, sandbox, executable)
}

#[test]
fn read_only_pad_policy_provisions_only_the_pinned_cli_exception() {
    let (directory, mut context, mut sandbox, executable) = fixture();
    let pad = directory.path().join("own-pad");
    fs::create_dir(&pad).unwrap();
    sandbox.intent = ExecutionIntent::ReadOnly;
    sandbox.runtime_mode = Some("read-only-with-writable-roots".into());
    sandbox.writable_roots = vec![plain_path(&pad.canonicalize().unwrap()).unwrap()];
    context.settings.mode = sandbox.runtime_mode.clone();
    context.settings.additional_directories = sandbox.writable_roots.clone();
    let before = sandbox.clone();
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    verify_files(&policy).unwrap();
    assert_eq!(sandbox, before);
    assert_eq!(
        fs::read(paths(&policy).2).unwrap(),
        format!(
            "prefix_rule(pattern={}, decision=\"allow\")\n",
            serde_json::to_string(&[&policy.executable]).unwrap()
        )
        .as_bytes()
    );
    assert!(fs::read_dir(pad).unwrap().next().is_none());
    remove_owned(&policy);
}

#[test]
fn provisions_only_fresh_protected_policy_and_exact_executable_prefix() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    assert_eq!(policy.tab_id, context.tab_id);
    assert_eq!(policy.binding_nonce, context.binding_nonce);
    assert_eq!(policy.epoch, context.epoch);
    assert_eq!(policy.metadata_directory, ".runtime-policy");
    assert_eq!(policy.executable_sha256, digest(b"opaque-cli-bytes"));
    verify_files(&policy).unwrap();
    let (directory, config, rule) = paths(&policy);
    let bytes = fs::read(&rule).unwrap();
    assert_eq!(bytes, rule_bytes(&policy.executable).unwrap());
    assert!(!String::from_utf8(bytes).unwrap().contains("identify"));
    assert!(provision(&context, &sandbox, &executable, ".runtime-policy").is_err());
    remove_owned(&policy);
    assert!(!directory.exists());
    assert!(!config.exists());
}

#[test]
fn metadata_directory_is_a_protected_direct_component_before_any_write() {
    let (_directory, context, sandbox, executable) = fixture();
    for directory in [
        "",
        ".",
        "..",
        ".git",
        ".GIT",
        "../foreign",
        ".own/foreign",
        ".own\\foreign",
        "C:/foreign",
        ".own:stream",
        ".own.",
    ] {
        assert!(
            provision(&context, &sandbox, &executable, directory).is_err(),
            "{directory}"
        );
        assert_eq!(fs::read_dir(&context.cwd).unwrap().count(), 0);
    }
}

#[test]
fn changed_executable_or_rule_or_new_project_configuration_refuses() {
    for target in ["executable", "rule", "config", "extra"] {
        let (_directory, context, sandbox, executable) = fixture();
        let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
        let (directory, config, rule) = paths(&policy);
        match target {
            "executable" => fs::write(&executable, b"different-cli-bytes").unwrap(),
            "rule" => fs::write(&rule, b"foreign-rule").unwrap(),
            "config" => fs::write(&config, b"foreign-project-hook-config").unwrap(),
            _ => fs::write(directory.join("rules/foreign.rules"), b"foreign").unwrap(),
        }
        assert!(verify_files(&policy).is_err(), "{target}");
    }
}

#[test]
fn foreign_metadata_writable_executable_and_changed_cwd_refuse_without_overwrite() {
    let (_directory, context, mut sandbox, executable) = fixture();
    sandbox
        .writable_roots
        .push(plain_path(executable.parent().unwrap()).unwrap());
    assert!(provision(&context, &sandbox, &executable, ".runtime-policy").is_err());
    sandbox.writable_roots.pop();
    let directory = Path::new(&context.cwd).join(".runtime-policy");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("config.toml"), b"foreign-config").unwrap();
    assert!(provision(&context, &sandbox, &executable, ".runtime-policy").is_err());
    assert_eq!(
        fs::read(directory.join("config.toml")).unwrap(),
        b"foreign-config"
    );
    fs::remove_file(directory.join("config.toml")).unwrap();
    fs::remove_dir(directory).unwrap();
    sandbox.cwd = plain_path(executable.parent().unwrap()).unwrap();
    assert!(provision(&context, &sandbox, &executable, ".runtime-policy").is_err());
}

#[test]
fn restored_session_under_provider_home_and_git_project_gets_fresh_scoped_policy() {
    let (directory, mut context, mut sandbox, executable) = fixture();
    for marker in [".codex", ".runtime-policy", ".git"] {
        fs::create_dir(directory.path().join(marker)).unwrap();
    }
    let foreign = directory.path().join(".runtime-policy/config.toml");
    fs::write(&foreign, b"foreign-ancestor-config").unwrap();
    let pad = directory.path().join("own-pad");
    fs::create_dir(&pad).unwrap();
    context.thread_id = Some("existing-session".into());
    context.settings.mode = Some("read-only-with-writable-roots".into());
    context.settings.additional_directories = vec![plain_path(&pad).unwrap()];
    sandbox.intent = ExecutionIntent::ReadOnly;
    sandbox.runtime_mode = context.settings.mode.clone();
    sandbox.writable_roots = context.settings.additional_directories.clone();
    for intent in [ExecutionIntent::ReadOnly, ExecutionIntent::Mutating] {
        if intent == ExecutionIntent::Mutating {
            context.settings.mode = Some("workspace-write".into());
            sandbox.runtime_mode = context.settings.mode.clone();
            sandbox.intent = intent;
            sandbox.writable_roots.push(context.cwd.clone());
            context.settings.additional_directories = sandbox.writable_roots.clone();
        }
        let before = context.clone();
        let before_sandbox = sandbox.clone();
        let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
        verify_files(&policy).unwrap();
        assert_eq!(context, before);
        assert_eq!(sandbox, before_sandbox);
        assert_eq!(fs::read(&foreign).unwrap(), b"foreign-ancestor-config");
        remove_owned(&policy);
        assert!(Path::new(&context.cwd).is_dir());
    }
}

#[test]
fn own_project_markers_refuse_without_changing_foreign_configuration() {
    for marker in [".runtime-policy", ".git"] {
        let (_directory, context, sandbox, executable) = fixture();
        let foreign = Path::new(&context.cwd).join(marker);
        fs::create_dir(&foreign).unwrap();
        let config = foreign.join("config.toml");
        fs::write(&config, b"foreign-project-config").unwrap();
        assert!(provision(&context, &sandbox, &executable, ".runtime-policy").is_err());
        assert_eq!(fs::read(config).unwrap(), b"foreign-project-config");
    }
}

#[test]
fn cleanup_preserves_foreign_changed_file() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    let (directory, _, rule) = paths(&policy);
    fs::write(&rule, b"foreign-rule").unwrap();
    remove_owned(&policy);
    assert_eq!(fs::read(rule).unwrap(), b"foreign-rule");
    assert!(directory.exists());
}

#[test]
fn changed_lifecycle_identity_refuses() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    assert!(binding_matches(&policy, &context));
    for changed in [
        RuntimeContext {
            epoch: context.epoch + 1,
            ..context.clone()
        },
        RuntimeContext {
            tab_id: "foreign".into(),
            ..context.clone()
        },
        RuntimeContext {
            binding_nonce: "foreign".into(),
            ..context.clone()
        },
        RuntimeContext {
            cwd: "foreign".into(),
            ..context.clone()
        },
    ] {
        assert!(!binding_matches(&policy, &changed));
    }
}

#[cfg(unix)]
#[test]
fn linked_policy_or_executable_refuses() {
    let (_directory, context, sandbox, executable) = fixture();
    let alias = executable.with_extension("alias");
    std::os::unix::fs::symlink(&executable, &alias).unwrap();
    assert!(provision(&context, &sandbox, &alias, ".runtime-policy").is_err());
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    let (_, config, rule) = paths(&policy);
    fs::remove_file(&rule).unwrap();
    std::os::unix::fs::symlink(config, rule).unwrap();
    assert!(verify_files(&policy).is_err());
}

#[test]
fn normal_runtime_shutdown_releases_matching_policy_without_erasing_saved_tab() {
    let (service, tab_id, _peer, _cleanup) = super::super::engine::test_support::runtime_fixture();
    let (_foreign_service, foreign_tab, _foreign_peer, _foreign_cleanup) =
        super::super::engine::test_support::runtime_fixture();
    let (_directory, mut context, sandbox, executable) = fixture();
    let session = super::super::engine::for_tab(&tab_id).unwrap();
    let binding = session.context.lock().clone();
    context.tab_id = binding.tab_id;
    context.binding_nonce = binding.binding_nonce;
    context.epoch = binding.epoch;
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    let (directory, _, _) = paths(&policy);
    LEASES.lock().insert(
        tab_id.clone(),
        Lease {
            policy,
            sandbox,
            provider: session.provider.clone(),
            _executable_pin: pin_executable(&executable).unwrap(),
        },
    );
    let before =
        serde_json::to_value(service.service.mux.read().get_tab(&tab_id).unwrap()).unwrap();
    super::super::engine::shutdown_blocking(&service.service).unwrap();
    assert!(super::super::engine::for_tab(&tab_id).is_none());
    assert!(super::super::engine::for_tab(&foreign_tab).is_some());
    assert!(!LEASES.lock().contains_key(&tab_id));
    assert!(!directory.exists());
    let after = serde_json::to_value(service.service.mux.read().get_tab(&tab_id).unwrap()).unwrap();
    assert_eq!(before, after);
    super::super::engine::shutdown_blocking(&service.service).unwrap();
}

#[test]
fn lease_revocation_reports_preserved_foreign_policy() {
    let (_directory, mut context, sandbox, executable) = fixture();
    context.tab_id = uuid::Uuid::new_v4().to_string();
    let policy = provision(&context, &sandbox, &executable, ".runtime-policy").unwrap();
    let (_, _, rule) = paths(&policy);
    LEASES.lock().insert(
        context.tab_id.clone(),
        Lease {
            policy,
            sandbox,
            provider: "opaque-provider".into(),
            _executable_pin: pin_executable(&executable).unwrap(),
        },
    );
    fs::write(&rule, b"foreign-policy").unwrap();
    assert!(revoke(&context.tab_id).is_err());
    assert_eq!(fs::read(rule).unwrap(), b"foreign-policy");
}

#[test]
fn trusted_cli_requires_literal_command_path_and_never_promotes_an_interpreter() {
    for path in [
        "C:/trusted/cli.exe",
        "/trusted/cli",
        "C:/USERNA~1/bin/cli.exe",
        "/trusted/ALIAS~1/cli",
    ] {
        assert!(literal_command_path(path));
    }
    for path in [
        "C:/space path/cli.exe",
        "& C:/trusted/cli.exe",
        "C:/trusted/cli.exe;pwsh",
        "C:/trusted/$env.exe",
        "~/cli.exe",
        "relative/ALIAS~1/cli",
        "C:ALIAS~1/cli.exe",
        "",
    ] {
        assert!(!literal_command_path(path));
    }
}

#[test]
fn spaced_cli_path_uses_only_a_verified_existing_literal_alias_or_refuses() {
    let directory = tempfile::tempdir().unwrap();
    let spaced = directory.path().join("space directory");
    fs::create_dir(&spaced).unwrap();
    let executable = spaced.join("trusted-cli.exe");
    fs::write(&executable, b"opaque-cli").unwrap();
    let canonical = executable.canonicalize().unwrap();
    match command_path(&canonical) {
        Ok(alias) => {
            assert!(literal_command_path(&alias));
            assert_eq!(Path::new(&alias).canonicalize().unwrap(), canonical);
            assert_eq!(
                file_digest(Path::new(&alias)).unwrap(),
                digest(b"opaque-cli")
            );
        }
        Err(reason) => assert!(reason.contains("literal")),
    }
    assert_eq!(fs::read(&executable).unwrap(), b"opaque-cli");
    assert!(!directory.path().join(".runtime-policy").exists());
}
