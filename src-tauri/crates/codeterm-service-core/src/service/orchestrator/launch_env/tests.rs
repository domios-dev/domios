use super::*;

#[test]
fn execution_environment_overrides_provider_values_and_removals() {
    let policy = codeterm_types::plan::ExecutionSandboxPolicy {
        intent: codeterm_types::plan::ExecutionIntent::Mutating,
        cwd: "lane".into(),
        launch_args: Vec::new(),
        runtime_mode: None,
        writable_roots: vec!["lane/temp".into()],
        environment: std::collections::BTreeMap::from([(
            "TMPDIR".into(),
            "lane/quoted 'temp".into(),
        )]),
    };
    let command = PreparedAgentCommand::from_spec(codeterm_types::agent::LaunchSpec {
        program: "fixture".into(),
        env_set: std::collections::HashMap::from([("TMPDIR".into(), "foreign".into())]),
        env_unset: vec!["TMPDIR".into(), "UNRELATED".into()],
        ..Default::default()
    })
    .with_execution_environment(Some(&policy));
    let direct = command.direct.unwrap();
    assert_eq!(direct.env_set["TMPDIR"], policy.environment["TMPDIR"]);
    assert_eq!(direct.env_unset, vec!["UNRELATED"]);
    let quoted = if cfg!(windows) {
        "lane/quoted ''temp"
    } else {
        "lane/quoted '\"'\"'temp"
    };
    assert!(command.shell_command.contains(quoted));
}

#[test]
fn quoted_direct_spec_retires_previous_settings_and_preserves_its_current_file() {
    let (mux, tab_id) = crate::multiplexer::state::MultiplexerState::new();
    let mut fixture = crate::service::test_fixture::ServiceFixture::new(mux);
    fixture.service.agent_pad_data_dir = fixture.temp.path().join("launch 'quoted' юникод 😀");
    let service = &fixture.service;
    let write = |body, suffix| {
        crate::service::task_files::capture(service, &tab_id, || {
            std::path::PathBuf::from(codeterm_util::task_files::write(body, Some(suffix)).unwrap())
        })
    };
    let previous = write("previous settings", ".ct-settings.json");
    let current = write("current settings", ".ct-settings.json");
    let task = write("opaque task", ".ct-task.md");
    let command = PreparedAgentCommand::from_spec(codeterm_types::agent::LaunchSpec {
        program: "fixture".into(),
        args: vec!["--settings".into(), current.to_str().unwrap().into()],
        ..Default::default()
    });
    assert_ne!(
        command.shell_command,
        format!("fixture --settings {}", current.display())
    );
    crate::service::task_files::retire_settings_for_launch(
        service,
        &tab_id,
        &command.shell_command,
    );
    assert!(!previous.exists());
    assert_eq!(
        std::fs::read_to_string(&current).unwrap(),
        "current settings"
    );
    assert_eq!(std::fs::read_to_string(&task).unwrap(), "opaque task");
    let retained = service
        .ext::<crate::service::task_files::LaunchTaskFiles>()
        .0
        .lock();
    assert_eq!(retained[&tab_id].len(), 2);
    assert!(retained[&tab_id].contains(&current));
    assert!(!retained[&tab_id].contains(&previous));
}

#[test]
fn direct_launch_environment_preserves_values_and_host_identity_without_shell_quoting() {
    use codeterm_types::agent::LaunchSpec;
    let cli_dir = crate::pty::manager::cli_dir_of(std::ffi::OsStr::new(cli_path())).unwrap();
    let path = std::env::join_paths([
        cli_dir.clone(),
        std::path::PathBuf::from("/fixture/bin"),
        cli_dir.clone(),
    ])
    .unwrap();
    let inherited = [
        ("PATH".into(), path.to_str().unwrap().into()),
        ("REMOVE_ME".into(), "inherited".into()),
    ]
    .into();
    let spec = LaunchSpec {
        env_set: [
            ("VALUE".into(), "юникод 😀 'quoted' $value; `raw`".into()),
            ("CODETERM_SESSION_BINDING_NONCE".into(), "spoof".into()),
        ]
        .into(),
        env_unset: vec!["REMOVE_ME".into()],
        ..Default::default()
    };
    let env = direct_environment(
        inherited,
        &spec,
        "nonce-'😀",
        "attempt-$raw",
        Some(std::path::Path::new(cli_path())),
        std::ffi::OsStr::new(""),
    )
    .unwrap();
    assert_eq!(env["VALUE"], spec.env_set["VALUE"]);
    assert_eq!(env["CODETERM_SESSION_BINDING_NONCE"], "nonce-'😀");
    assert_eq!(env["CODETERM_STARTUP_ATTEMPT_ID"], "attempt-$raw");
    assert!(!env.contains_key("REMOVE_ME"));
    assert_eq!(occurrences(std::ffi::OsStr::new(&env["PATH"]), &cli_dir), 1);
    assert_eq!(std::env::split_paths(&env["PATH"]).next(), Some(cli_dir));
    let wrapped = command_with_binding_nonce_for_shell(
        "fixture",
        "nonce-'😀",
        "attempt-$raw",
        cfg!(windows),
        Some(std::ffi::OsStr::new(cli_path())),
    );
    assert!(wrapped.contains("CODETERM_SESSION_BINDING_NONCE="));
    assert!(wrapped.contains("CODETERM_STARTUP_ATTEMPT_ID="));
    assert_ne!(
        env["VALUE"],
        codeterm_util::shell::ShellSyntax::quote(&env["VALUE"])
    );
    for key in [
        "CODETERM_SESSION_BINDING_NONCE",
        "CODETERM_STARTUP_ATTEMPT_ID",
        "PATH",
    ] {
        let invalid = LaunchSpec {
            env_unset: vec![key.into()],
            ..Default::default()
        };
        assert!(direct_environment(
            Default::default(),
            &invalid,
            "n",
            "a",
            None,
            std::ffi::OsStr::new("")
        )
        .is_err());
    }
}

fn cli_path() -> &'static str {
    if cfg!(windows) {
        r"C:\ct\bin\codeterm.exe"
    } else {
        "/opt/ct/bin/codeterm"
    }
}

fn occurrences(path: &std::ffi::OsStr, dir: &std::path::Path) -> usize {
    std::env::split_paths(path)
        .filter(|entry| entry.as_path() == dir)
        .count()
}

/// End to end across both owners of the prepend: the PTY layer stamps PATH, the
/// shell profile then re-adds the CLI directory, and the launch line runs last.
/// The launched tab must carry the directory exactly once — a count, not a
/// string, because the failure this pins is a duplicate entry.
#[test]
fn launched_tab_carries_the_cli_dir_exactly_once() {
    let cli_dir = crate::pty::manager::cli_dir_of(std::ffi::OsStr::new(cli_path())).unwrap();
    let inherited: Vec<std::path::PathBuf> = if cfg!(windows) {
        vec![r"C:\Windows\system32".into(), r"C:\Users\u\bin".into()]
    } else {
        vec!["/usr/bin".into(), "/opt/homebrew/bin".into()]
    };
    let base = std::env::join_paths(&inherited).unwrap();

    let spawned = crate::pty::manager::path_with_cli_dir_first(&base, &cli_dir)
        .expect("pty layer stamps PATH");
    assert_eq!(occurrences(&spawned, &cli_dir), 1);

    // The shell profile re-adds its own copy after the PTY env is fixed — the
    // drift the launch line exists to correct.
    let after_profile = std::env::join_paths(
        std::env::split_paths(&spawned).chain(std::iter::once(cli_dir.clone())),
    )
    .unwrap();
    assert_eq!(
        occurrences(&after_profile, &cli_dir),
        2,
        "fixture must reproduce the profile drift the launch line corrects"
    );

    let launched = crate::pty::manager::path_with_cli_dir_first(&after_profile, &cli_dir)
        .expect("launch line stamps PATH");

    assert_eq!(
        occurrences(&launched, &cli_dir),
        1,
        "a launched tab must carry the CLI dir exactly once"
    );
    assert_eq!(
        std::env::split_paths(&launched).next().as_deref(),
        Some(cli_dir.as_path()),
        "the launch line owns precedence after profile loading"
    );
    let survivors: Vec<std::path::PathBuf> = std::env::split_paths(&launched)
        .filter(|entry| entry.as_path() != cli_dir)
        .collect();
    assert_eq!(survivors, inherited, "no inherited entry may be disturbed");
}

/// The prelude must name the directory twice — once as the prepended value and
/// once as the entry it filters out. The old unfiltered `PATH='DIR;' + PATH`
/// named it once, which is exactly the shape that duplicated the entry.
#[test]
fn launch_line_removes_existing_copies_before_prepending() {
    for windows in [true, false] {
        let dir = if windows { r"C:\ct\bin" } else { "/opt/ct/bin" };
        let prelude = if windows {
            powershell_cli_dir_prelude(dir)
        } else {
            posix_cli_dir_prelude(dir)
        };
        assert_eq!(
            prelude.matches(dir).count(),
            2,
            "prelude must both prepend and filter (windows={windows}): {prelude}"
        );
    }
}

/// Companion to the two tests above: without a configured CLI path the launch
/// line must not touch PATH at all. An implementation that always stamps would
/// pass the count assertions and still corrupt a tab that has no CLI.
#[test]
fn launch_line_leaves_path_alone_without_a_configured_cli() {
    for windows in [true, false] {
        let command =
            command_with_binding_nonce_for_shell("claude", "nonce", "pty-a", windows, None);
        assert!(
            !command.contains("PATH"),
            "no CLI path must mean no PATH statement (windows={windows}): {command}"
        );
        assert!(command.ends_with("claude"));
    }
}

/// The configured directory still reaches the line, and the agent command stays
/// last so the prelude never swallows it.
#[test]
fn configured_cli_directory_is_restored_after_shell_profile_loading() {
    let command = command_with_binding_nonce_for_shell(
        "claude",
        "nonce",
        "pty-a",
        false,
        Some(std::ffi::OsStr::new("/dev/bin/codeterm")),
    );
    assert!(command.starts_with(
        "export CODETERM_SESSION_BINDING_NONCE='nonce'; export CODETERM_STARTUP_ATTEMPT_ID='pty-a'; export PATH="
    ));
    assert!(command.contains("'/dev/bin'"));
    assert!(command.ends_with("; claude"));
}

#[test]
fn dev_instance_agents_resolve_the_instance_cli_first() {
    let instance_cli_dir = if cfg!(windows) {
        std::path::PathBuf::from(r"C:\Users\u\.domios-dev-worker-a\bin")
    } else {
        std::path::PathBuf::from("/home/u/.domios-dev-worker-a/bin")
    };
    let prod_cli_dir = if cfg!(windows) {
        std::path::PathBuf::from(r"C:\Users\u\.domios\bin")
    } else {
        std::path::PathBuf::from("/home/u/.domios/bin")
    };
    let inherited = std::env::join_paths([prod_cli_dir.clone()]).unwrap();

    let launched = crate::pty::manager::path_with_cli_dir_first(&inherited, &instance_cli_dir)
        .expect("active instance CLI path is prepended");

    assert_eq!(
        std::env::split_paths(&launched).next().as_deref(),
        Some(instance_cli_dir.as_path())
    );
    assert!(std::env::split_paths(&launched).any(|entry| entry == prod_cli_dir));
}

#[test]
fn direct_launch_specs_cannot_replace_or_remove_tab_credentials() {
    use codeterm_types::agent::LaunchSpec;
    for key in ["CODETERM_API_TOKEN", "CODETERM_TAB_ID"] {
        for spec in [
            LaunchSpec {env_set: [(key.into(), "foreign".into())].into(), ..Default::default()},
            LaunchSpec {env_unset: vec![key.into()], ..Default::default()},
        ] {
            assert!(direct_environment(Default::default(), &spec, "nonce", "attempt", None, std::ffi::OsStr::new("")).unwrap_err().contains("tab credentials"));
        }
    }
}
