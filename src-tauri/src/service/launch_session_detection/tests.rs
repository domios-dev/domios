use super::*;

fn git(cwd: &std::path::Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    clear_git_environment(&mut command);
    let output = command
        .current_dir(cwd)
        .args(args)
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn lane_git_writable_roots_derived_from_real_worktree() {
    let temp = tempfile::tempdir().expect("temporary repository");
    let repo = temp.path().join("repo");
    let lane = temp.path().join("lane");
    std::fs::create_dir(&repo).expect("repository directory");
    git(&repo, &["init"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=CodeTerm",
            "-c",
            "user.email=codeterm@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/worker",
            lane.to_str().unwrap(),
        ],
    );

    let roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
        .expect("valid lane metadata")
        .expect("linked worktree");
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    clear_git_environment(&mut command);
    let output = command
        .current_dir(&lane)
        .args(["rev-parse", "--git-dir"])
        .output()
        .expect("git reports lane gitdir");
    let lane_gitdir = String::from_utf8(output.stdout).expect("path is UTF-8");
    let lane_gitdir = std::path::Path::new(lane_gitdir.trim());
    let lane_gitdir = if lane_gitdir.is_absolute() {
        lane_gitdir.to_path_buf()
    } else {
        lane.join(lane_gitdir)
    };
    let expected_lane_gitdir =
        canonical_path_from_path(&lane_gitdir).expect("lane gitdir resolves");
    assert_eq!(roots[0], expected_lane_gitdir);
    assert!(roots[0].join("commondir").is_file());
    assert_ne!(roots[0], repo);
    let common = canonical_path_from_path(&repo.join(".git")).expect("common directory");
    let worktrees = common.join("worktrees");
    assert_eq!(roots[0].parent(), Some(worktrees.as_path()));
    assert_eq!(roots[1], common.join("objects"));
    assert_eq!(roots[2], common.join("refs"));
    assert_eq!(roots[3], common.join("logs"));
}

#[test]
fn crafted_gitfile_pointing_at_another_lane_is_refused() {
    let temp = tempfile::tempdir().expect("temporary repository");
    let repo = temp.path().join("repo");
    let lane = temp.path().join("lane");
    let forged = temp.path().join("forged-lane");
    std::fs::create_dir(&repo).expect("repository directory");
    git(&repo, &["init"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=CodeTerm",
            "-c",
            "user.email=codeterm@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/worker",
            lane.to_str().unwrap(),
        ],
    );
    std::fs::create_dir(&forged).expect("forged worktree directory");
    std::fs::write(
        forged.join(".git"),
        std::fs::read_to_string(lane.join(".git")).expect("lane gitfile"),
    )
    .expect("crafted gitfile");

    assert!(git_worktree_metadata_dirs(forged.to_str().unwrap(), repo.to_str().unwrap(),).is_err());
}

#[test]
fn ordinary_repository_has_no_additional_git_metadata_roots() {
    let temp = tempfile::tempdir().expect("temporary repository");
    git(temp.path(), &["init"]);
    assert!(git_worktree_metadata_dirs(
        temp.path().to_str().unwrap(),
        temp.path().to_str().unwrap(),
    )
    .expect("valid ordinary repository")
    .is_none());
}

#[test]
fn sandboxed_launch_grants_only_its_own_agent_pad() {
    let data = tempfile::tempdir().expect("data directory");
    let pad = crate::service::agent_pad::AgentPad::create(
        data.path(),
        "tab-pad-1",
        crate::service::agent_pad::AgentPadRole::Worker,
    )
    .expect("pad");
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture")
            .with_additional_writable_dir_flag("--add-dir"),
    ));
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "no-flag",
    )));

    let mut sandboxed = Vec::new();
    append_sandbox_agent_pad_arg(
        &registry,
        "fixture",
        false,
        Some(data.path().to_str().unwrap()),
        data.path(),
        "tab-pad-1",
        &mut sandboxed,
    );
    let expected = canonical_path_from_path(pad.path()).expect("pad resolves");
    assert_eq!(
        sandboxed,
        vec![
            "--add-dir".to_string(),
            expected.to_string_lossy().into_owned()
        ]
    );

    let mut full_access = Vec::new();
    append_sandbox_agent_pad_arg(
        &registry,
        "fixture",
        true,
        Some(data.path().to_str().unwrap()),
        data.path(),
        "tab-pad-1",
        &mut full_access,
    );
    assert!(full_access.is_empty());
    let mut undeclared = Vec::new();
    append_sandbox_agent_pad_arg(
        &registry,
        "no-flag",
        false,
        Some(data.path().to_str().unwrap()),
        data.path(),
        "tab-pad-1",
        &mut undeclared,
    );
    assert!(undeclared.is_empty());
    let mut created = Vec::new();
    append_sandbox_agent_pad_arg(
        &registry,
        "fixture",
        false,
        Some(data.path().to_str().unwrap()),
        data.path(),
        "tab-without-pad",
        &mut created,
    );
    let created_pad = crate::service::agent_pad::AgentPad::open(data.path(), "tab-without-pad")
        .expect("launch creates the missing pad");
    let created_path = canonical_path_from_path(created_pad.path()).expect("pad resolves");
    assert_eq!(
        created,
        vec![
            "--add-dir".to_string(),
            created_path.to_string_lossy().into_owned()
        ]
    );
}

#[test]
fn sandboxed_lane_launch_includes_git_roots_only_for_lanes() {
    let temp = tempfile::tempdir().expect("temporary repository");
    let repo = temp.path().join("repo");
    let lane = temp.path().join("lane");
    std::fs::create_dir(&repo).expect("repository directory");
    git(&repo, &["init"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=CodeTerm",
            "-c",
            "user.email=codeterm@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/worker",
            lane.to_str().unwrap(),
        ],
    );
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture")
            .with_additional_writable_dir_flag("--add-dir"),
    ));

    let roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
        .expect("valid lane metadata")
        .expect("linked worktree");
    let mut args = Vec::new();
    append_sandbox_git_metadata_args(
        &registry,
        "fixture",
        false,
        Some(lane.to_str().unwrap()),
        Some(repo.to_str().unwrap()),
        "metadata-test-tab",
        &mut args,
    );
    assert_eq!(
        args,
        vec![
            "--add-dir".to_string(),
            roots[0].to_string_lossy().into_owned(),
            "--add-dir".to_string(),
            roots[1].to_string_lossy().into_owned(),
            "--add-dir".to_string(),
            roots[2].to_string_lossy().into_owned(),
            "--add-dir".to_string(),
            roots[3].to_string_lossy().into_owned(),
        ]
    );
    assert!(args.iter().all(|arg| arg != repo.to_str().unwrap()));
    assert!(args.iter().all(|arg| arg != lane.to_str().unwrap()));
    assert!(args
        .iter()
        .all(|arg| arg != repo.join(".git").to_str().unwrap()));

    let mut full_access_args = vec!["--existing".to_string()];
    append_sandbox_git_metadata_args(
        &registry,
        "fixture",
        true,
        Some(lane.to_str().unwrap()),
        Some(repo.to_str().unwrap()),
        "metadata-test-tab",
        &mut full_access_args,
    );
    assert_eq!(full_access_args, vec!["--existing".to_string()]);

    let ordinary = temp.path().join("ordinary");
    std::fs::create_dir(&ordinary).expect("ordinary repository directory");
    git(&ordinary, &["init"]);
    let mut ordinary_args = vec!["--existing".to_string()];
    append_sandbox_git_metadata_args(
        &registry,
        "fixture",
        false,
        Some(ordinary.to_str().unwrap()),
        Some(ordinary.to_str().unwrap()),
        "metadata-test-tab",
        &mut ordinary_args,
    );
    assert_eq!(ordinary_args, vec!["--existing".to_string()]);
}

#[test]
fn a_verbatim_cwd_resolves_the_same_directory_trust_as_its_plain_spelling() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::new("fixture")
            .with_trusted_directory("C:\\repo"),
    ));
    assert!(sandbox_additional_writable_dirs_allowed(
        &registry,
        "fixture",
        Some("\\\\?\\C:\\repo"),
        "agent_pad",
    ));
    assert!(!sandbox_additional_writable_dirs_allowed(
        &registry,
        "fixture",
        Some("C:\\other"),
        "agent_pad",
    ));
}

#[test]
fn sandboxed_additional_roots_follow_provider_directory_trust_state() {
    let temp = tempfile::tempdir().expect("temporary directory");
    let data = temp.path().join("data");
    std::fs::create_dir(&data).expect("data directory");
    let pad = crate::service::agent_pad::AgentPad::create(
        &data,
        "trust-state-tab",
        crate::service::agent_pad::AgentPadRole::General,
    )
    .expect("pad");
    let repo = temp.path().join("repo");
    let lane = temp.path().join("lane");
    std::fs::create_dir(&repo).expect("repository directory");
    git(&repo, &["init"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=CodeTerm",
            "-c",
            "user.email=codeterm@example.invalid",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-b",
            "lane/trust-state",
            lane.to_str().unwrap(),
        ],
    );
    let git_roots = git_worktree_metadata_dirs(lane.to_str().unwrap(), repo.to_str().unwrap())
        .expect("valid lane metadata")
        .expect("linked worktree");

    for (state, expected_count) in [
        (codeterm_plugin::provider::DirectoryTrustState::Trusted, 10),
        (codeterm_plugin::provider::DirectoryTrustState::Untrusted, 0),
        (codeterm_plugin::provider::DirectoryTrustState::Unknown, 0),
    ] {
        let registry = crate::agent::registry::AgentRegistry::new();
        registry.register_provider(Box::new(
            crate::agent::test_provider::MockProvider::new("fixture")
                .with_additional_writable_dir_flag("--add-dir")
                .with_directory_trust_state(state),
        ));
        let mut args = Vec::new();
        append_sandbox_agent_pad_arg(
            &registry,
            "fixture",
            false,
            Some(lane.to_str().unwrap()),
            &data,
            "trust-state-tab",
            &mut args,
        );
        append_sandbox_git_metadata_args(
            &registry,
            "fixture",
            false,
            Some(lane.to_str().unwrap()),
            Some(repo.to_str().unwrap()),
            "trust-state-tab",
            &mut args,
        );
        assert_eq!(args.len(), expected_count);
        if state == codeterm_plugin::provider::DirectoryTrustState::Trusted {
            let expected_pad = canonical_path_from_path(pad.path()).expect("pad resolves");
            assert_eq!(args[1], expected_pad.to_string_lossy());
            assert_eq!(args[3], git_roots[0].to_string_lossy());
            assert_eq!(args[5], git_roots[1].to_string_lossy());
        }
    }
}

#[test]
fn detection_sources_journal_as_their_own_binding_source() {
    assert_eq!(
        binding_source(LaunchSessionSource::PidRegistry),
        BindingSource::PidRegistry
    );
    assert_eq!(
        binding_source(LaunchSessionSource::LaunchMarker),
        BindingSource::LaunchMarker
    );
    assert_ne!(
        binding_source(LaunchSessionSource::PidRegistry),
        BindingSource::Hook
    );
}

#[test]
fn marked_agent_command_carries_the_marker_its_tab_currently_holds() {
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "marked",
    )));
    let params = crate::service::SpawnAgentParams {
        agent: Some("marked".into()),
        ..Default::default()
    };

    let first = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
    let first_marker = registered_launch_marker("tab-marked").expect("first marker");
    assert!(!first_marker.is_empty());
    assert_eq!(first, format!("marked-cli --launch-marker {first_marker}"));

    let relaunch = marked_agent_command("tab-marked", "marked", &params, None, None, &registry);
    let relaunch_marker = registered_launch_marker("tab-marked").expect("relaunch marker");
    assert_ne!(relaunch_marker, first_marker);
    assert_eq!(
        relaunch,
        format!("marked-cli --launch-marker {relaunch_marker}")
    );
}

#[test]
fn marked_provider_launch_command_hands_every_fresh_launch_its_tabs_marker() {
    use crate::agent::provider::AgentLaunchParams;

    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(crate::agent::test_provider::MockProvider::new(
        "direct",
    )));
    let cases = [
        ("tab-direct-plain", AgentLaunchParams::default()),
        (
            "tab-direct-task",
            AgentLaunchParams {
                task: Some("fixture task".into()),
                ..Default::default()
            },
        ),
        (
            "tab-direct-injected",
            AgentLaunchParams {
                session_id: Some("injected-session".into()),
                ..Default::default()
            },
        ),
    ];

    for (tab_id, mut params) in cases {
        let command =
            marked_provider_launch_command(&registry, "direct", tab_id, None, None, &mut params)
                .expect("registered provider builds a command");
        let marker = registered_launch_marker(tab_id).expect("tab registers its marker");
        assert_eq!(params.launch_marker.as_deref(), Some(marker.as_str()));
        assert_eq!(command, format!("direct-cli --launch-marker {marker}"));
    }
}

#[test]
fn mark_fresh_launch_registers_the_marker_it_hands_the_params() {
    let mut params = crate::agent::provider::AgentLaunchParams::default();
    mark_fresh_launch("tab-mark-only", &mut params);
    let marker = registered_launch_marker("tab-mark-only").expect("registered marker");
    assert!(!marker.is_empty());
    assert_eq!(params.launch_marker, Some(marker));
}

#[test]
fn sandbox_additional_roots_logged_when_included() {
    assert_eq!(
        sandbox_additional_roots_log("tab-1", 3).as_deref(),
        Some("[sandbox-additional-roots] included 3 roots for tab tab-1")
    );
    assert_eq!(sandbox_additional_roots_log("tab-1", 0), None);
}

#[test]
fn resume_target_is_exact_and_belongs_to_the_current_launch() {
    let (mut mux, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = mux.get_tab_mut(&id).unwrap();
    tab.session_binding_nonce = Some("current".into());
    tab.prior_session_binding_nonces
        .push(crate::multiplexer::types::PriorBindingNonce {
            nonce: "current".into(),
            session_id: None,
            resume_target_session_id: Some("own".into()),
        });
    assert_eq!(resumed_session_target(tab, Some("own")), Some("own"));
    assert_eq!(resumed_session_target(tab, Some("newest-global")), None);
    assert_eq!(resumed_session_target(tab, None), None);
    tab.session_binding_nonce = Some("replacement".into());
    assert_eq!(resumed_session_target(tab, Some("own")), None);
    tab.agent_session_id = Some("foreign".into());
    assert_eq!(resumed_session_target(tab, Some("own")), None);
    assert_eq!(
        resumed_session_target(tab, Some("foreign")),
        Some("foreign")
    );
}

#[test]
fn resumed_session_requires_exact_provider_store_evidence() {
    assert!(resume_session_binding_allowed(Some(true)));
    assert!(!resume_session_binding_allowed(Some(false)));
    assert!(!resume_session_binding_allowed(None));
}

#[test]
fn plain_pty_resume_binds_its_claimed_session_without_a_sidecar() {
    let (mut state, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("file-backed-fixture".into());
    tab.pty_id = Some("own-pty".into());
    tab.session_binding_nonce = Some("current-launch".into());
    tab.remember_resume_binding_nonce("saved-session");
    let target = resumed_session_target(tab, Some("saved-session"))
        .unwrap()
        .to_string();
    assert!(resume_session_binding_allowed(Some(true)));
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "plain-resume",
        state,
    ));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(
        crate::agent::test_provider::MockProvider::with_session(
            "file-backed-fixture",
            "saved-session",
        ),
    ));
    let claim = crate::api::hook_routes::bind_detected_session(
        &mux,
        &registry,
        &id,
        "file-backed-fixture",
        &target,
        "current-launch",
    )
    .unwrap();
    assert!(claim.nonce_verified);
    let state = mux.read();
    let tab = state.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("saved-session"));
    assert!(tab.launch_session_authenticated());
}

#[test]
fn sqlite_resume_binds_only_the_claimed_session_without_a_session_file() {
    use crate::agent::provider::AgentProvider;
    struct SqliteStoreProvider(parking_lot::Mutex<rusqlite::Connection>);
    impl AgentProvider for SqliteStoreProvider {
        fn id(&self) -> &str {
            "sqlite-fixture"
        }
        fn display_name(&self) -> &str {
            "SQLite fixture"
        }
        fn build_launch_command(&self, _: &crate::agent::provider::AgentLaunchParams) -> String {
            String::new()
        }
        fn readiness_strategy(&self) -> crate::agent::provider::ReadinessStrategy {
            crate::agent::provider::ReadinessStrategy::PollShellPrompt {
                timeout: std::time::Duration::ZERO,
            }
        }
        fn task_delivery(&self) -> crate::agent::provider::TaskDelivery {
            crate::agent::provider::TaskDelivery::PtyInput
        }
        fn detect_from_title(&self, _: &str) -> bool {
            false
        }
        fn detect_from_output(&self, _: &str) -> bool {
            false
        }
        fn build_resume_command(&self, _: &str, _: bool) -> Option<String> {
            None
        }
        fn session_file_path(&self, _: &str, _: &str) -> Option<std::path::PathBuf> {
            None
        }
        fn detect_session_id(
            &self,
            _: &str,
            _: &[String],
            _: Option<std::time::Duration>,
        ) -> Option<String> {
            None
        }
        fn session_exists(&self, _: &str, id: &str) -> Option<bool> {
            self.0
                .lock()
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM session WHERE id = ?1)",
                    [id],
                    |row| row.get(0),
                )
                .ok()
        }
    }
    let store = rusqlite::Connection::open_in_memory().unwrap();
    store.execute_batch("CREATE TABLE session (id TEXT PRIMARY KEY); INSERT INTO session VALUES ('own'), ('newest-sibling');").unwrap();
    let provider = SqliteStoreProvider(parking_lot::Mutex::new(store));
    assert!(provider.session_file_path("/work", "own").is_none());
    assert!(provider_owns_resume_session(&provider, "/work", "own"));
    assert!(!provider_owns_resume_session(&provider, "/work", "missing"));
    let (mut state, id) = crate::multiplexer::state::MultiplexerState::new();
    let tab = state.get_tab_mut(&id).unwrap();
    tab.agent_type = Some("sqlite-fixture".into());
    tab.pty_id = Some("own-pty".into());
    tab.cwd = Some("/work".into());
    tab.session_binding_nonce = Some("current-launch".into());
    tab.remember_resume_binding_nonce("own");
    assert_eq!(resumed_session_target(tab, Some("own")), Some("own"));
    assert_eq!(resumed_session_target(tab, Some("newest-sibling")), None);
    let mux = std::sync::Arc::new(codeterm_util::lock_hold::TimedRwLock::new(
        "sqlite-resume",
        state,
    ));
    let registry = crate::agent::registry::AgentRegistry::new();
    registry.register_provider(Box::new(provider));
    let claim = crate::api::hook_routes::bind_detected_session(
        &mux,
        &registry,
        &id,
        "sqlite-fixture",
        "own",
        "current-launch",
    )
    .unwrap();
    assert!(claim.nonce_verified);
    let mux = mux.read();
    let tab = mux.get_tab(&id).unwrap();
    assert_eq!(tab.agent_session_id.as_deref(), Some("own"));
    assert!(tab.launch_session_authenticated());
}

#[test]
fn pty_and_runtime_host_roots_share_order_and_exact_deduplication() {
    let root = std::env::current_dir().unwrap();
    let pad = root.join("pad");
    let metadata = [
        root.join("worktree"),
        root.join("objects"),
        root.join("refs"),
        root.join("logs"),
    ];
    let combined = combine_host_writable_roots(Some(pad.clone()), Some(metadata.clone()));
    let separately = combine_host_writable_roots(Some(pad.clone()), None)
        .into_iter()
        .chain(combine_host_writable_roots(None, Some(metadata.clone())))
        .collect::<Vec<_>>();
    assert_eq!(combined, separately);
    let duplicate = [
        pad.clone(),
        metadata[1].clone(),
        metadata[2].clone(),
        metadata[3].clone(),
    ];
    assert_eq!(
        combine_host_writable_roots(Some(pad), Some(duplicate)).len(),
        4
    );
}

#[test]
fn pty_and_runtime_linux_network_policy_keeps_existing_access_and_host_scope() {
    assert_eq!(
        sandbox_network_access_policy(false, "linux", true, false),
        Some(true)
    );
    assert_eq!(
        sandbox_network_access_policy(false, "linux", false, true),
        Some(true)
    );
    assert_eq!(
        sandbox_network_access_policy(false, "linux", false, false),
        Some(false)
    );
    assert_eq!(
        sandbox_network_access_policy(true, "linux", true, true),
        None
    );
    assert_eq!(
        sandbox_network_access_policy(false, "macos", true, false),
        None
    );
    assert_eq!(
        sandbox_network_access_policy(false, "windows", true, false),
        None
    );
}
