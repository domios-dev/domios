use super::*;

fn entry(pid: u32, parent_pid: u32) -> ProcessEntry {
    ProcessEntry {
        pid,
        parent_pid,
        image_name: String::new(),
        argv0_basename: None,
        script_basename: None,
    }
}

fn named_entry(pid: u32, parent_pid: u32, image_name: &str) -> ProcessEntry {
    ProcessEntry {
        pid,
        parent_pid,
        image_name: image_name.into(),
        argv0_basename: None,
        script_basename: None,
    }
}

fn tree_key() -> ProviderTreeKey {
    ProviderTreeKey {
        tab_id: "tab-tree-witness".into(),
        pty_id: "pty-tree-witness".into(),
        binding_nonce: Some("nonce-tree-witness".into()),
        executable_name: "opencode".into(),
    }
}

fn provider_process(pid: u32) -> HashSet<ProcessIdentity> {
    HashSet::from([ProcessIdentity {
        pid,
        start_token: format!("start-{pid}"),
    }])
}

fn pids(tree: &[LaunchProcess]) -> Vec<(u32, u32)> {
    tree.iter()
        .map(|process| (process.pid, process.depth))
        .collect()
}

#[test]
fn tree_lists_only_the_roots_descendants_shallowest_first() {
    let entries = [entry(20, 10), entry(30, 20), entry(40, 99), entry(21, 10)];
    let tree = launch_tree(10, &entries, |pid| Some(u64::from(pid)));
    assert_eq!(pids(&tree), vec![(10, 0), (20, 1), (21, 1), (30, 2)]);
    assert_eq!(tree[1].start_token, "20");
}

#[test]
fn child_older_than_its_parent_is_a_reused_pid_and_is_excluded() {
    let entries = [entry(20, 10), entry(30, 20)];
    let starts = |pid: u32| match pid {
        10 => Some(500),
        20 => Some(100),
        _ => Some(900),
    };
    assert_eq!(pids(&launch_tree(10, &entries, starts)), vec![(10, 0)]);
}

#[test]
fn provider_process_must_be_named_and_descend_from_the_tab_root() {
    let entries = [
        named_entry(10, 1, "powershell.exe"),
        named_entry(20, 10, "codex.exe"),
        named_entry(30, 1, "codex.exe"),
        named_entry(40, 1, "powershell.exe"),
    ];

    assert!(tree_has_named_process(
        10,
        &entries,
        |pid| Some(pid.into()),
        "codex"
    ));
    assert!(!tree_has_named_process(
        40,
        &entries,
        |pid| Some(pid.into()),
        "codex"
    ));
}

#[test]
fn named_process_lookup_is_scoped_to_the_tab_tree_and_normalizes_exe_suffix() {
    let entries = [
        named_entry(10, 1, "pwsh.exe"),
        named_entry(20, 10, "opencode.exe"),
        named_entry(30, 99, "pwsh.exe"),
        named_entry(40, 99, "opencode.exe"),
    ];
    let has_process = |root, expected| {
        launch_tree(root, &entries, |pid| Some(u64::from(pid)))
            .iter()
            .any(|process| {
                entries
                    .iter()
                    .find(|entry| entry.pid == process.pid)
                    .is_some_and(|entry| process_name_matches(entry, expected))
            })
    };

    assert!(has_process(10, "opencode"));
    assert!(has_process(10, "OPENCODE.EXE"));
    assert!(!has_process(30, "opencode"));
}

#[test]
fn provider_absent_before_first_seen_is_not_exit_evidence() {
    let witnesses = ProviderTreeWitnesses::default();
    let key = tree_key();
    let first_absence = witnesses.record(&key, ProviderProcessTreeSnapshot::Absent);

    assert_eq!(first_absence.state, ProviderTreeState::AbsentBeforeSeen);
    assert!(!first_absence.proves_exit_from_lifecycle_clear());
    assert!(!first_absence.proves_exit_from_finished_marker());

    let live = witnesses.record(
        &key,
        ProviderProcessTreeSnapshot::Present(provider_process(77)),
    );
    assert_eq!(live.state, ProviderTreeState::Present);
    assert!(!live.proves_exit_from_lifecycle_clear());

    let one_absence = witnesses.record(&key, ProviderProcessTreeSnapshot::Absent);
    assert_eq!(one_absence.state, ProviderTreeState::GoneAfterSeen);
    assert!(one_absence.proves_exit_from_finished_marker());
    assert!(one_absence.proves_exit_from_lifecycle_clear());
}

#[test]
fn live_provider_observation_is_seen_before_later_absence() {
    let witnesses = ProviderTreeWitnesses::default();
    let key = tree_key();
    let live = witnesses.record(
        &key,
        ProviderProcessTreeSnapshot::Present(provider_process(78)),
    );

    assert_eq!(live.state(), ProviderTreeState::Present);
    assert!(witnesses.has_seen(&key));

    let absent = witnesses.record(&key, ProviderProcessTreeSnapshot::Absent);
    assert_eq!(absent.state(), ProviderTreeState::GoneAfterSeen);
    assert_ne!(absent.state(), ProviderTreeState::AbsentBeforeSeen);
}

#[test]
fn incomplete_tree_and_live_provider_are_not_exit_evidence() {
    let entries = [
        named_entry(10, 1, "powershell.exe"),
        named_entry(20, 10, "opencode.exe"),
    ];
    assert!(matches!(
        named_process_snapshot_from_entries(10, &entries, |pid| Some(u64::from(pid)), "opencode"),
        ProviderProcessTreeSnapshot::Present(processes) if processes.contains(&ProcessIdentity { pid: 20, start_token: "20".into() })
    ));
    assert!(matches!(
        named_process_snapshot_from_entries(10, &[], |_| None, "opencode"),
        ProviderProcessTreeSnapshot::Unknown
    ));
}

#[test]
fn missing_root_or_child_identity_is_conservatively_live() {
    let entries = [
        named_entry(10, 1, "pwsh.exe"),
        named_entry(20, 10, "opencode.exe"),
    ];
    assert!(tree_may_have_named_process(
        10,
        &entries,
        |pid| { Some(u64::from(pid)) },
        "opencode"
    ));
    assert!(!tree_may_have_named_process(
        10,
        &entries,
        |pid| { Some(u64::from(pid)) },
        "codex"
    ));
    assert!(tree_may_have_named_process(10, &[], |_| None, "opencode"));
    assert!(tree_may_have_named_process(
        10,
        &entries,
        |pid| { (pid != 20).then_some(u64::from(pid)) },
        "opencode"
    ));
    assert!(!tree_may_have_named_process(
        10,
        &entries,
        |pid| Some(u64::from(pid)),
        "codex"
    ));
}

#[test]
fn process_without_a_start_time_is_excluded_with_its_subtree() {
    let entries = [entry(20, 10), entry(30, 20)];
    let starts = |pid: u32| (pid != 20).then_some(u64::from(pid));
    assert_eq!(pids(&launch_tree(10, &entries, starts)), vec![(10, 0)]);
}

#[test]
fn parent_links_that_cycle_terminate() {
    let entries = [entry(10, 20), entry(20, 10)];
    let tree = launch_tree(10, &entries, |_| Some(1));
    assert_eq!(pids(&tree), vec![(10, 0), (20, 1)]);
}

#[test]
fn unknown_root_yields_no_tree() {
    assert!(launch_tree(10, &[entry(20, 10)], |_| None).is_empty());
}

#[test]
fn launcher_and_versioned_images_match_the_exact_argv0_basename() {
    for image in ["agent-cli-macos-aarch64", "agent-cli-1.0.50"] {
        let mut provider = named_entry(20, 10, image);
        provider.argv0_basename = Some("agent-cli".into());
        let entries = [named_entry(10, 1, "zsh"), provider];
        assert!(tree_has_named_process(
            10,
            &entries,
            |pid| Some(pid.into()),
            "agent-cli"
        ));
        assert!(matches!(
            named_process_snapshot_from_entries(10, &entries, |pid| Some(pid.into()), "agent-cli"),
            ProviderProcessTreeSnapshot::Present(processes) if processes.contains(&ProcessIdentity { pid: 20, start_token: "20".into() })
        ));
        assert!(matches!(
            named_process_snapshot_from_entries(
                10,
                &entries,
                |pid| (pid != 20).then_some(pid.into()),
                "agent-cli"
            ),
            ProviderProcessTreeSnapshot::Unknown
        ));
        assert!(!tree_has_named_process(
            99,
            &entries,
            |pid| Some(pid.into()),
            "agent-cli"
        ));
    }
}

#[test]
fn unrelated_argv0_and_image_prefixes_do_not_match() {
    let mut provider = named_entry(20, 10, "agent-cli-macos-aarch64");
    provider.argv0_basename = Some("other-agent-cli".into());
    let entries = [named_entry(10, 1, "zsh"), provider];
    assert!(!tree_has_named_process(
        10,
        &entries,
        |pid| Some(pid.into()),
        "agent-cli"
    ));
    assert!(matches!(
        named_process_snapshot_from_entries(10, &entries, |pid| Some(pid.into()), "agent-cli"),
        ProviderProcessTreeSnapshot::Absent
    ));
}

#[test]
fn windows_executable_matching_keeps_image_name_and_exe_normalization() {
    let provider = named_entry(20, 10, "C:\\bin\\agent-cli.EXE");
    assert!(process_name_matches(&provider, "agent-cli"));
    assert!(process_name_matches(&provider, "AGENT-CLI.exe"));
    assert!(!process_name_matches(&provider, "other"));
}

#[test]
fn image_name_matches_when_argv0_is_unavailable_or_different() {
    let mut provider = named_entry(20, 10, "agent-cli");
    assert!(process_name_matches(&provider, "agent-cli"));
    provider.argv0_basename = Some("other".into());
    assert!(process_name_matches(&provider, "agent-cli"));
}

#[test]
fn shebang_script_matches_only_in_its_own_tab_tree() {
    let names = argv0::names_from_cmdline(b"/venv/bin/python3.12\0-u\0/venv/bin/aider\0").unwrap();
    let mut provider = named_entry(20, 10, "Python");
    provider.argv0_basename = Some(names.argv0_basename);
    provider.script_basename = names.script_basename;
    assert!(process_name_matches(&provider, "aider"));
    assert!(!process_name_matches(&provider, "other"));
    let entries = [
        named_entry(10, 1, "zsh"),
        provider,
        named_entry(30, 1, "zsh"),
    ];
    assert!(tree_has_named_process(
        10,
        &entries,
        |pid| Some(pid.into()),
        "aider"
    ));
    assert!(!tree_has_named_process(
        30,
        &entries,
        |pid| Some(pid.into()),
        "aider"
    ));
    assert!(matches!(
        named_process_snapshot_from_entries(10, &entries, |pid| Some(pid.into()), "aider"),
        ProviderProcessTreeSnapshot::Present(processes) if processes.contains(&ProcessIdentity { pid: 20, start_token: "20".into() })
    ));
}

#[test]
fn python_running_another_script_does_not_match_provider_argument() {
    let names = argv0::names_from_cmdline(b"python\0other.py\0aider\0").unwrap();
    let mut provider = named_entry(20, 10, "python");
    provider.argv0_basename = Some(names.argv0_basename);
    provider.script_basename = names.script_basename;
    assert!(!process_name_matches(&provider, "aider"));
    let entries = [named_entry(10, 1, "zsh"), provider];
    assert!(!tree_has_named_process(
        10,
        &entries,
        |pid| Some(pid.into()),
        "aider"
    ));
}
