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
