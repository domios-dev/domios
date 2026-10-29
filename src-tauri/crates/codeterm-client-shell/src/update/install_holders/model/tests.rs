use super::*;

fn holder(pid: u32, parent_pid: Option<u32>, path: Option<&str>) -> Holder {
    Holder {
        pid,
        name: format!("process-{pid}"),
        executable: path.map(PathBuf::from),
        parent_pid,
        owned_tab: None,
    }
}

#[test]
fn standalone_installed_cli_is_owned_but_names_and_other_installs_are_not() {
    let clis = [PathBuf::from(r"C:\Domios\cli\codeterm.exe")];
    assert_eq!(
        classify_holder(
            &holder(2, Some(90), Some(r"\\?\c:\DOMIOS\cli\codeterm.exe")),
            1,
            &clis,
            &[]
        ),
        HolderDisposition::OwnedHelper
    );
    for path in [
        r"C:\Other\cli\codeterm.exe",
        r"C:\Domios\codeterm.exe",
        r"C:\Domios\cli\foreign.exe",
    ] {
        assert_eq!(
            classify_holder(&holder(2, Some(1), Some(path)), 1, &clis, &[]),
            HolderDisposition::Foreign
        );
    }
}

#[test]
fn consoles_require_both_exact_path_and_current_instance_parent() {
    let helpers = [
        PathBuf::from(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        PathBuf::from(r"C:\Domios\conpty\arm64\OpenConsole.exe"),
    ];
    for path in &helpers {
        let path = path.to_str().unwrap();
        assert_eq!(
            classify_holder(&holder(2, Some(1), Some(path)), 1, &[], &helpers),
            HolderDisposition::OwnedHelper
        );
        for parent in [Some(90), None] {
            assert_eq!(
                classify_holder(&holder(2, parent, Some(path)), 1, &[], &helpers),
                HolderDisposition::Foreign
            );
        }
    }
    assert_eq!(
        classify_holder(&holder(2, Some(1), None), 1, &[], &helpers),
        HolderDisposition::Foreign
    );
}

#[test]
fn updater_is_never_terminated_and_unknown_holders_are_foreign() {
    assert_eq!(
        classify_holder(&holder(1, None, None), 1, &[], &[]),
        HolderDisposition::UpdatingApp
    );
    assert_eq!(
        classify_holder(
            &holder(2, Some(1), Some(r"C:\Domios\foreign.exe")),
            1,
            &[],
            &[]
        ),
        HolderDisposition::Foreign
    );
}

#[test]
fn reused_pid_is_not_the_restart_manager_process() {
    assert!(same_process_identity((1, 2), (1, 2)));
    assert!(!same_process_identity((1, 2), (1, 3)));
    assert!(!same_process_identity((1, 2), (2, 2)));
}

#[test]
fn notice_names_every_foreign_holder_and_pid_and_retry() {
    let holders = [holder(22, None, None), holder(83, None, None)];
    assert_eq!(foreign_notice(&holders), "Update blocked by install-file holders: process-22 (PID 22); process-83 (PID 83). Close these processes and choose Retry.");
}

#[test]
fn pinned_job_membership_proves_ownership_even_with_unknown_image() {
    let mut member = holder(2, Some(90), None);
    member.owned_tab = Some("Tab Euler (tab-1)".into());
    assert_eq!(
        classify_holder(&member, 1, &[], &[]),
        HolderDisposition::OwnedTab
    );
}

#[test]
fn restart_manager_set_is_only_install_binaries_beside_thousands_of_foreign_files() {
    let root = PathBuf::from("install");
    let expected = [
        "domios-app.exe",
        "codeterm-embed.exe",
        "WebView2Loader.DLL",
        "cli/codeterm.exe",
        "cli/codeterm-x86_64-pc-windows-msvc.exe",
        "domios-server/domios-server.exe",
        "conpty/x64/OpenConsole.exe",
        "conpty/x64/conpty.dll",
        "conpty/arm64/OpenConsole.exe",
        "conpty/arm64/conpty.dll",
    ]
    .map(|path| root.join(path));
    let mut listing = expected.to_vec();
    for index in 0..5000 {
        listing.push(root.join(format!("dist/assets/chunk-{index}.js")));
        listing.push(root.join(format!("skills/skill-{index}/SKILL.md")));
        listing.push(root.join(format!("build-gateway/jobs/job-{index}.log")));
        listing.push(root.join(format!(
            "build-gateway/targets/release/job-{index}/release/domios-app.exe"
        )));
        listing.push(root.join(format!(
            "build-gateway/targets/release/job-{index}/release/deps/codeterm.dll"
        )));
    }
    for path in [
        "server-component.json",
        "LICENSE",
        "cli/codeterm.exe.old",
        "cli/.gitkeep",
        "skills/tool/bin/helper.exe",
        "domios-server/plugins/foreign.dll",
        "conpty/OpenConsole.exe",
        "other/install/domios-app.exe",
    ] {
        listing.push(root.join(path));
    }
    listing.push(PathBuf::from("elsewhere/domios-app.exe"));
    assert_eq!(install_binaries(&root, listing), expected.to_vec());
}

#[test]
fn only_exact_attached_server_is_exempt_from_preflight() {
    let server = crate::application_host::AttachedServerIdentity {
        pid: 42,
        created: "12345".into(),
    };
    assert!(is_attached_server(Some(&server), 42, 12345));
    assert!(!is_attached_server(Some(&server), 43, 12345));
    assert!(!is_attached_server(Some(&server), 42, 12346));
    assert!(!is_attached_server(None, 42, 12345));
    let foreign = Holder {
        pid: 43,
        name: "opaque foreign holder".into(),
        executable: Some(PathBuf::from("/foreign.exe")),
        parent_pid: None,
        owned_tab: None,
    };
    assert_eq!(
        classify_holder(&foreign, 1, &[], &[]),
        HolderDisposition::Foreign
    );
    assert!(foreign_notice(&[foreign]).contains("PID 43"));
}

fn server_processes() -> HashMap<u32, ProcessRecord> {
    [
        (
            42,
            ProcessRecord {
                created: 100,
                parent_pid: 1,
                executable: Some(PathBuf::from(r"C:\Domios\domios-server\domios-server.exe")),
            },
        ),
        (
            43,
            ProcessRecord {
                created: 200,
                parent_pid: 42,
                executable: Some(PathBuf::from(r"C:\Domios\domios-server\domios-server.exe")),
            },
        ),
        (
            44,
            ProcessRecord {
                created: 300,
                parent_pid: 43,
                executable: Some(PathBuf::from(r"C:\Domios\conpty\x64\OpenConsole.exe")),
            },
        ),
    ]
    .into()
}

fn server_owns(
    pid: u32,
    created: u64,
    image: Option<&str>,
    processes: &HashMap<u32, ProcessRecord>,
) -> bool {
    let server = crate::application_host::AttachedServerIdentity {
        pid: 42,
        created: "100".into(),
    };
    let images = [
        PathBuf::from(r"C:\Domios\domios-server\domios-server.exe"),
        PathBuf::from(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        PathBuf::from(r"C:\Domios\conpty\arm64\OpenConsole.exe"),
    ];
    is_owned_server_holder(
        &server,
        pid,
        created,
        image.map(Path::new),
        &images,
        processes,
    )
}

#[test]
fn exact_server_owns_pty_shims_and_conpty_descendants() {
    let processes = server_processes();
    assert!(server_owns(
        42,
        100,
        Some(r"C:\Domios\domios-server\domios-server.exe"),
        &processes
    ));
    assert!(server_owns(
        43,
        200,
        Some(r"C:\Domios\domios-server\domios-server.exe"),
        &processes
    ));
    for arch in ["x64", "arm64"] {
        assert!(server_owns(
            44,
            300,
            Some(&format!(r"\\?\c:\DOMIOS\conpty\{arch}\OpenConsole.exe")),
            &processes
        ));
    }
}

#[test]
fn foreign_install_server_still_blocks_even_under_an_owned_tab() {
    assert!(!server_owns(
        43,
        200,
        Some(r"C:\Developer\Domios\target\debug\domios-server.exe"),
        &server_processes()
    ));
    assert!(!server_owns(
        44,
        300,
        Some(r"C:\Other\conpty\x64\OpenConsole.exe"),
        &server_processes()
    ));
    let mut processes = server_processes();
    processes.get_mut(&43).unwrap().executable = Some(PathBuf::from(
        r"C:\Developer\Domios\target\debug\domios-server.exe",
    ));
    assert!(!server_owns(
        44,
        300,
        Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        &processes
    ));
}

#[test]
fn same_image_without_proven_ancestry_is_foreign() {
    for pid in [43, 44] {
        let mut processes = server_processes();
        processes.remove(&42);
        assert!(!server_owns(
            pid,
            if pid == 43 { 200 } else { 300 },
            Some(r"C:\Domios\domios-server\domios-server.exe"),
            &processes
        ));
    }
    let mut processes = server_processes();
    processes.get_mut(&43).unwrap().parent_pid = 99;
    assert!(!server_owns(
        44,
        300,
        Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        &processes
    ));
    assert!(!server_owns(43, 200, None, &server_processes()));
}

#[test]
fn reused_server_or_intermediate_or_holder_pid_never_proves_ownership() {
    for (pid, replacement_created) in [(42, 101), (43, 400), (44, 301)] {
        let mut processes = server_processes();
        processes.get_mut(&pid).unwrap().created = replacement_created;
        assert!(!server_owns(
            44,
            300,
            Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
            &processes
        ));
    }
    assert!(!server_owns(
        44,
        301,
        Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        &server_processes()
    ));
}

#[test]
fn foreign_tree_and_cyclic_or_incomplete_ancestry_remain_blockers() {
    for parent_pid in [1, 44, 99] {
        let mut processes = server_processes();
        processes.get_mut(&43).unwrap().parent_pid = parent_pid;
        assert!(!server_owns(
            44,
            300,
            Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
            &processes
        ));
    }
    let mut processes = server_processes();
    processes.remove(&43);
    assert!(!server_owns(
        44,
        300,
        Some(r"C:\Domios\conpty\x64\OpenConsole.exe"),
        &processes
    ));
}
