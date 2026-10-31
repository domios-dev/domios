use super::*;
use codeterm_plugin::agent_runtime::RuntimeSettings;

fn fixture() -> (
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
fn provisions_only_fresh_protected_policy_and_exact_executable_prefix() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable).unwrap();
    assert_eq!(policy.tab_id, context.tab_id);
    assert_eq!(policy.binding_nonce, context.binding_nonce);
    assert_eq!(policy.epoch, context.epoch);
    assert_eq!(policy.executable_sha256, digest(b"opaque-cli-bytes"));
    verify_files(&policy).unwrap();
    let (directory, config, rule) = paths(&policy);
    let bytes = fs::read(&rule).unwrap();
    assert_eq!(bytes, rule_bytes(&policy.executable).unwrap());
    assert!(!String::from_utf8(bytes).unwrap().contains("identify"));
    assert!(provision(&context, &sandbox, &executable).is_err());
    remove_owned(&policy);
    assert!(!directory.exists());
    assert!(!config.exists());
}

#[test]
fn changed_executable_or_rule_or_new_project_configuration_refuses() {
    for target in ["executable", "rule", "config", "extra"] {
        let (_directory, context, sandbox, executable) = fixture();
        let policy = provision(&context, &sandbox, &executable).unwrap();
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
    assert!(provision(&context, &sandbox, &executable).is_err());
    sandbox.writable_roots.pop();
    let directory = Path::new(&context.cwd).join(".codex");
    fs::create_dir(&directory).unwrap();
    fs::write(directory.join("config.toml"), b"foreign-config").unwrap();
    assert!(provision(&context, &sandbox, &executable).is_err());
    assert_eq!(
        fs::read(directory.join("config.toml")).unwrap(),
        b"foreign-config"
    );
    fs::remove_file(directory.join("config.toml")).unwrap();
    fs::remove_dir(directory).unwrap();
    sandbox.cwd = plain_path(executable.parent().unwrap()).unwrap();
    assert!(provision(&context, &sandbox, &executable).is_err());
}

#[test]
fn foreign_ancestor_project_metadata_refuses_trust() {
    let (directory, context, _, _) = fixture();
    fs::create_dir(directory.path().join(".codex")).unwrap();
    assert!(require_fresh_project(Path::new(&context.cwd)).is_err());
}

#[test]
fn cleanup_preserves_foreign_changed_file() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable).unwrap();
    let (directory, _, rule) = paths(&policy);
    fs::write(&rule, b"foreign-rule").unwrap();
    remove_owned(&policy);
    assert_eq!(fs::read(rule).unwrap(), b"foreign-rule");
    assert!(directory.exists());
}

#[test]
fn changed_lifecycle_identity_refuses() {
    let (_directory, context, sandbox, executable) = fixture();
    let policy = provision(&context, &sandbox, &executable).unwrap();
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
    assert!(provision(&context, &sandbox, &alias).is_err());
    let policy = provision(&context, &sandbox, &executable).unwrap();
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
    let policy = provision(&context, &sandbox, &executable).unwrap();
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
    let policy = provision(&context, &sandbox, &executable).unwrap();
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
fn full_concierge_skips_optional_exception_only_with_current_host_bypass_grant() {
    use crate::multiplexer::{state::MultiplexerState, types::NeighborAuthority};
    use crate::service::{test_fixture::ServiceFixture, SpawnAgentParams};
    use codeterm_neighbors::permissions::MachineAccess;

    let (_directory, mut context, policy, _executable) = fixture();
    let (mut mux, tab_id) = MultiplexerState::new();
    context.tab_id = tab_id.clone();
    context.settings.mode = Some("full-access".into());
    let authority = NeighborAuthority {
        uid: "owner".into(),
        friendship_id: "a".repeat(64),
        root_tab_id: None,
    };
    {
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.agent_type = Some("opaque-pty".into());
        tab.session_binding_nonce = Some(context.binding_nonce.clone());
        tab.neighbor_authority = Some(authority.clone());
    }
    let service = ServiceFixture::new(mux);
    service.register_neighbor_advisory_provider("opaque-pty");
    let svc = &service.service;
    svc.set_neighbor_friend_receiver_for_test(&authority.friendship_id, Some(true), 1);
    // A full-looking reported mode cannot replace missing confined launch authority.
    assert!(sandbox(svc, &context).is_err());
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .permission_bypass_requested = true;
    assert!(sandbox(svc, &context).is_err());
    let mut params = SpawnAgentParams {
        neighbor_authority: Some(authority.clone()),
        ..Default::default()
    };
    svc.configure_neighbor_launch_access("opaque-pty", MachineAccess::Full, &mut params)
        .unwrap();
    svc.prepare_neighbor_restore_grant(&tab_id, "opaque-pty", &params)
        .unwrap();
    assert_eq!(sandbox(svc, &context).unwrap(), None);
    assert!(!Path::new(&context.cwd).join(".codex").exists());
    {
        let mut mux = svc.mux.write();
        mux.get_tab_mut(&tab_id)
            .unwrap()
            .restorable_launch
            .execution_sandbox = Some(policy);
    }
    assert!(sandbox(svc, &context).is_err());
    svc.mux
        .write()
        .get_tab_mut(&tab_id)
        .unwrap()
        .restorable_launch
        .execution_sandbox = None;
    let mut foreign = context.clone();
    foreign.binding_nonce = "foreign-binding".into();
    assert!(sandbox(svc, &foreign).is_err());
    svc.set_neighbor_friend_receiver_for_test(&authority.friendship_id, Some(false), 2);
    assert!(sandbox(svc, &context).is_err());
}

#[test]
fn trusted_cli_requires_literal_command_path_and_never_promotes_an_interpreter() {
    for path in [
        "C:/trusted/cli.exe",
        "/trusted/cli",
        "C:/USERNA~1/bin/cli.exe",
    ] {
        assert!(literal_command_path(path));
    }
    for path in [
        "C:/space path/cli.exe",
        "& C:/trusted/cli.exe",
        "C:/trusted/cli.exe;pwsh",
        "C:/trusted/$env.exe",
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
    assert!(!directory.path().join(".codex").exists());
}
